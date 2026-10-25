defmodule LeashCollector.JsonlTailProducerTest do
  @moduledoc """
  Tests the Broadway file-tail producer in isolation (ADR 0003): audit-only
  emission, cursor persistence/resume, and truncation replay.
  """

  use ExUnit.Case, async: true

  alias LeashCollector.JsonlTailProducer

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
    dir = System.tmp_dir!() |> Path.join("leash-tail-#{System.unique_integer([:positive])}")
    File.mkdir_p!(dir)
    on_exit(fn -> File.rm_rf!(dir) end)
    Path.join(dir, name)
  end

  defp write!(path, lines) do
    File.write!(path, Enum.join(lines, "\n") <> "\n")
  end

  # Collect up to `n` messages with a timeout so tests do not block forever
  # when the producer has nothing more to emit.
  defp collect(producer, n, timeout \\ 1000) do
    task =
      Task.async(fn ->
        [producer]
        |> GenStage.stream()
        |> Enum.take(n)
      end)

    case Task.yield(task, timeout) do
      {:ok, events} ->
        events

      nil ->
        Task.shutdown(task, :brutal_kill)
        # Give the producer time to process the subscription cancel (monitor
        # DOWN) so a just-killed consumer cannot swallow a later emission.
        Process.sleep(25)
        []
    end
  end

  defp request_ids(events) do
    Enum.map(events, & &1.data["request_id"])
  end

  defp start_producer!(path, cursor_path) do
    {:ok, pid} =
      JsonlTailProducer.start_link(path: path, cursor_path: cursor_path, poll_interval: 10)

    pid
  end

  defp read_cursor(cursor_path) do
    assert {:ok, raw} = File.read(cursor_path)
    assert {:ok, cursor} = Jason.decode(raw)
    cursor
  end

  test "emits only audit lines and consumes skipped ones" do
    path = tmp_path("audit.jsonl")
    cursor = tmp_path("audit.cursor")
    write!(path, [op_line("connect"), audit_line("1"), op_line("close"), audit_line("2")])

    producer = start_producer!(path, cursor)

    assert request_ids(collect(producer, 2)) == ["req-1", "req-2"]
    # Cursor covers all four lines: skipped operational lines are consumed
    # so they are not re-examined on every poll.
    assert read_cursor(cursor)["offset"] == byte_size(File.read!(path))
  end

  test "resumes from the cursor on restart without replaying" do
    path = tmp_path("audit.jsonl")
    cursor = tmp_path("audit.cursor")
    write!(path, [audit_line("1"), audit_line("2")])

    producer = start_producer!(path, cursor)
    assert request_ids(collect(producer, 1)) == ["req-1"]
    assert read_cursor(cursor)["offset"] > 0

    # Restart with the same cursor: nothing is replayed...
    producer2 = start_producer!(path, cursor)
    assert [] = collect(producer2, 1, 200)

    # ...and newly appended lines are picked up.
    File.write!(path, File.read!(path) <> audit_line("3") <> "\n")
    assert request_ids(collect(producer2, 1)) == ["req-3"]
  end

  test "replays the file from byte 0 after truncation" do
    path = tmp_path("audit.jsonl")
    cursor = tmp_path("audit.cursor")
    write!(path, [audit_line("1"), audit_line("2")])

    producer = start_producer!(path, cursor)
    assert request_ids(collect(producer, 2)) == ["req-1", "req-2"]

    # Truncate and write shorter content: the old cursor is invalid, so the
    # whole file is replayed (idempotent inserts make this safe).
    File.write!(path, audit_line("short") <> "\n")
    assert request_ids(collect(producer, 1)) == ["req-short"]
  end

  test "does not consume an incomplete trailing line" do
    path = tmp_path("audit.jsonl")
    cursor = tmp_path("audit.cursor")
    write!(path, [audit_line("1")])
    # Simulate a partially flushed record still being written (no newline).
    File.write!(path, audit_line("2"), [:append])

    producer = start_producer!(path, cursor)

    assert request_ids(collect(producer, 1)) == ["req-1"]
    assert [] = collect(producer, 1, 100)

    # Once the line is terminated, it is picked up.
    File.write!(path, "\n", [:append])
    assert request_ids(collect(producer, 1)) == ["req-2"]
    assert read_cursor(cursor)["offset"] == byte_size(File.read!(path))
  end

  test "waits quietly when the log file does not exist yet" do
    path = tmp_path("audit.jsonl")
    cursor = tmp_path("audit.cursor")

    producer = start_producer!(path, cursor)
    assert [] = collect(producer, 1, 100)

    write!(path, [audit_line("1")])
    assert request_ids(collect(producer, 1)) == ["req-1"]
  end
end
