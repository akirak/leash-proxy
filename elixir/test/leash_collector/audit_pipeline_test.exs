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
    start_supervised!(
      {AuditPipeline,
       name: AuditPipelineTest.Pipeline,
       producer: [
         module: {JsonlTailProducer, [path: path, cursor_path: cursor, poll_interval: 10]}
       ],
       batchers: [audit_events: [concurrency: 1, batch_size: 200, batch_timeout: 20]]}
    )
  end

  test "a replacement producer can deliver JSON payloads with duplicates and invalid records" do
    pipeline = LeashCollector.AuditPipelineTest.BrokerPipeline

    start_supervised!(
      {AuditPipeline,
       name: pipeline,
       producer: [module: {Broadway.DummyProducer, []}],
       batchers: [audit_events: [concurrency: 1, batch_size: 200, batch_timeout: 20]]}
    )

    LeashCollector.Audit.subscribe()

    ref =
      Broadway.test_batch(pipeline, [
        audit_line("broker"),
        audit_line("broker"),
        audit_line("invalid", %{"status" => "bad"}),
        "not JSON"
      ])

    assert_receive {:ack, ^ref, successful, []}, 2_000
    assert length(successful) == 4
    assert_receive {:audit_events_inserted, 1}
    assert Repo.aggregate(LeashCollector.Audit.Event, :count) == 1
  end

  test "database failures produce failed acknowledgements and a replay succeeds" do
    pipeline = LeashCollector.AuditPipelineTest.RetryPipeline

    start_supervised!(
      {AuditPipeline,
       name: pipeline,
       producer: [module: {Broadway.DummyProducer, []}],
       batchers: [audit_events: [concurrency: 1, batch_size: 1, batch_timeout: 20]]}
    )

    Repo.query!(
      "CREATE TEMP TRIGGER reject_audit BEFORE INSERT ON audit_events BEGIN SELECT RAISE(FAIL, 'sink unavailable'); END"
    )

    ExUnit.CaptureLog.capture_log(fn ->
      ref = Broadway.test_message(pipeline, audit_line("retry"))
      assert_receive {:ack, ^ref, [], [_failed]}, 2_000
    end)

    assert Repo.aggregate(LeashCollector.Audit.Event, :count) == 0

    Repo.query!("DROP TRIGGER reject_audit")
    ref = Broadway.test_message(pipeline, audit_line("retry"))
    assert_receive {:ack, ^ref, [_successful], []}, 2_000
    assert Repo.get_by!(LeashCollector.Audit.Event, request_id: "req-retry")
  end

  test "tails the JSONL log into SQLite, skipping non-audit lines" do
    path = tmp_path("audit.jsonl")
    cursor = tmp_path("audit.cursor")

    File.write!(path, Enum.join([op_line("up"), audit_line("1"), op_line("down")], "\n") <> "\n")

    LeashCollector.Audit.subscribe()
    start_pipeline!(path, cursor)

    assert_receive {:audit_events_inserted, 1}, 2_000
    assert Repo.aggregate(LeashCollector.Audit.Event, :count) == 1

    event = Repo.get_by!(LeashCollector.Audit.Event, request_id: "req-1")
    assert event.host == "api.github.com"
    assert event.decision == "allow"
    assert event.agent_id == "agent-1"
    assert event.status == 0

    # New appends flow through the pipeline while it is running.
    File.write!(path, audit_line("2", %{"status" => 500}) <> "\n", [:append])

    assert_receive {:audit_events_inserted, 1}, 2_000
    assert Repo.get_by!(LeashCollector.Audit.Event, request_id: "req-2").status == 500
  end
end
