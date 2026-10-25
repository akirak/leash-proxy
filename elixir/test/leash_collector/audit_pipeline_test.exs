defmodule LeashCollector.AuditPipelineTest do
  @moduledoc """
  Integration test for the full Broadway topology: producer → processors →
  batcher → SQLite store, with real message routing and batching.
  """

  use ExUnit.Case, async: false

  alias LeashCollector.AuditPipeline
  alias LeashCollector.JsonlTailProducer
  alias LeashCollector.Repo

  # Share the sandbox transaction with the Broadway processor/batcher
  # processes so their inserts are visible to this test.
  setup do
    pid = Ecto.Adapters.SQL.Sandbox.start_owner!(LeashCollector.Repo, shared: true)
    on_exit(fn -> Ecto.Adapters.SQL.Sandbox.stop_owner(pid) end)
    :ok
  end

  defp audit_line(id, extra \\ %{}) do
    Jason.encode!(
      Map.merge(
        %{
          "timestamp" => "2026-10-25T12:00:00Z",
          "target" => "leash_proxy::audit",
          "request_id" => "req-#{id}",
          "agent_id" => "agent-1",
          "method" => "GET",
          "scheme" => "https",
          "host" => "api.github.com",
          "decision" => "allow"
        },
        Map.new(extra)
      )
    )
  end

  defp op_line(message) do
    Jason.encode!(%{
      "timestamp" => "2026-10-25T12:00:00Z",
      "target" => "leash_proxy::proxy",
      "message" => message
    })
  end

  defp tmp_path(name) do
    dir = System.tmp_dir!() |> Path.join("leash-pipe-#{System.unique_integer([:positive])}")
    File.mkdir_p!(dir)
    on_exit(fn -> File.rm_rf!(dir) end)
    Path.join(dir, name)
  end

  defp start_pipeline!(path, cursor) do
    {:ok, pid} =
      AuditPipeline.start_link(
        name: AuditPipelineTest.Pipeline,
        producer: [
          module: {JsonlTailProducer, [path: path, cursor_path: cursor, poll_interval: 10]}
        ]
      )

    pid
  end

  defp eventually(fun, timeout \\ 5_000) do
    deadline = System.monotonic_time(:millisecond) + timeout

    if fun.() do
      :ok
    else
      if System.monotonic_time(:millisecond) > deadline do
        flunk("condition not met within #{timeout}ms")
      else
        Process.sleep(50)
        eventually(fun, deadline - System.monotonic_time(:millisecond))
      end
    end
  end

  test "tails the JSONL log into SQLite, skipping non-audit lines" do
    path = tmp_path("audit.jsonl")
    cursor = tmp_path("audit.cursor")

    File.write!(path, Enum.join([op_line("up"), audit_line("1"), op_line("down")], "\n") <> "\n")

    start_pipeline!(path, cursor)

    assert eventually(fn -> Repo.aggregate(LeashCollector.Audit.Event, :count) == 1 end)

    event = Repo.get_by!(LeashCollector.Audit.Event, request_id: "req-1")
    assert event.host == "api.github.com"
    assert event.decision == "allow"
    assert event.agent_id == "agent-1"
    assert event.status == 0

    # New appends flow through the pipeline while it is running.
    File.write!(path, audit_line("2", %{"status" => 500}) <> "\n", [:append])

    assert eventually(fn ->
             event2 = Repo.get_by(LeashCollector.Audit.Event, request_id: "req-2")
             event2 != nil and event2.status == 500
           end)
  end
end
