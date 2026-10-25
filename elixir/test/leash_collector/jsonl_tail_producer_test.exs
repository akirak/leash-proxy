defmodule LeashCollector.JsonlTailProducerTest do
  use ExUnit.Case, async: true

  alias LeashCollector.JsonlTailProducer, as: Producer

  setup do
    dir = Path.join(System.tmp_dir!(), "leash-tail-#{System.unique_integer([:positive])}")
    File.mkdir_p!(dir)
    on_exit(fn -> File.rm_rf!(dir) end)
    path = Path.join(dir, "audit.jsonl")
    cursor = Path.join(dir, "cursor")
    {:producer, state} = Producer.init(path: path, cursor_path: cursor)
    {:ok, path: path, cursor: cursor, state: state}
  end

  defp line(id), do: Jason.encode!(%{target: "leash_proxy::audit", request_id: id}) <> "\n"

  defp poll(state, demand \\ 10) do
    {:noreply, [], state} = Producer.handle_demand(demand, state)
    {:noreply, events, state} = Producer.handle_info(:poll, state)
    {events, state}
  end

  defp acknowledge(events, state, failed \\ []) do
    [{_, ref, _} | _] = Enum.map(events ++ failed, & &1.acknowledger)
    :ok = Producer.ack(ref, events, failed)
    assert_receive {:ack, token, good, bad}
    {:noreply, [], state} = Producer.handle_info({:ack, token, good, bad}, state)
    state
  end

  test "checkpoints only after acknowledgement and resumes", %{
    path: path,
    cursor: cursor,
    state: state
  } do
    File.write!(path, line("one") <> line("two"))
    {[one, two], state} = poll(state)
    refute File.exists?(cursor)
    assert {[], _} = poll(state)
    state = acknowledge([one], state)
    refute File.exists?(cursor)
    _state = acknowledge([two], state)
    assert Jason.decode!(File.read!(cursor))["offset"] == byte_size(File.read!(path))

    {:producer, resumed} = Producer.init(path: path, cursor_path: cursor)
    assert {[], resumed} = poll(resumed)
    File.write!(path, line("three"), [:append])
    assert {[%{data: %{"request_id" => "three"}}], _} = poll(resumed)
  end

  test "unacknowledged messages are replayed after restart", %{
    path: path,
    cursor: cursor,
    state: state
  } do
    File.write!(path, line("one"))
    assert {[_], _} = poll(state)
    {:producer, resumed} = Producer.init(path: path, cursor_path: cursor)
    assert {[%{data: %{"request_id" => "one"}}], _} = poll(resumed)
  end

  test "failed chunks are replayed, including successful messages", %{path: path, state: state} do
    File.write!(path, line("one") <> line("two"))
    {[one, two], state} = poll(state)
    state = acknowledge([two], state, [one])

    assert {[%{data: %{"request_id" => "one"}}, %{data: %{"request_id" => "two"}}], _} =
             poll(state)
  end

  test "respects demand and counts blank and non-audit lines in byte offsets", %{
    path: path,
    cursor: cursor,
    state: state
  } do
    File.write!(path, "\n{}\ninvalid\n" <> line("one") <> line("two"))
    {[one], state} = poll(state, 1)
    assert one.data["request_id"] == "one"
    state = acknowledge([one], state)

    assert Jason.decode!(File.read!(cursor))["offset"] ==
             byte_size("\n{}\ninvalid\n" <> line("one"))

    assert {[%{data: %{"request_id" => "two"}}], _} = poll(state, 1)
  end

  test "waits for newline and for a missing file", %{path: path, state: state} do
    assert {[], state} = poll(state)
    File.write!(path, String.trim_trailing(line("one")))
    assert {[], state} = poll(state)
    File.write!(path, "\n", [:append])
    assert {[%{data: %{"request_id" => "one"}}], _} = poll(state)
  end

  test "replays on truncation and rotation", %{path: path, state: state} do
    File.write!(path, line("long-request-id"))
    {events, state} = poll(state)
    state = acknowledge(events, state)
    File.write!(path, line("short"))
    {events, state} = poll(state)
    assert [%{data: %{"request_id" => "short"}}] = events
    state = acknowledge(events, state)
    File.rename!(path, path <> ".old")
    File.write!(path, line("rotated"))
    assert {[%{data: %{"request_id" => "rotated"}}], _} = poll(state)
  end

  test "lines spanning read chunks do not wedge ingestion", %{path: path, state: state} do
    File.write!(path, line(String.duplicate("x", 300)))
    state = %{state | max_bytes: 64, read_bytes: 64}
    assert {[], state} = poll(state)
    assert {[], state} = poll(state)
    assert {[], state} = poll(state)
    assert {[%{data: %{"request_id" => id}}], _} = poll(state)
    assert byte_size(id) == 300
  end

  test "oversized lines are discarded through newline before resuming", %{
    path: path,
    cursor: cursor,
    state: state
  } do
    File.write!(path, String.duplicate("x", 200) <> "\n" <> line("good"))
    state = %{state | max_bytes: 64, read_bytes: 64, max_line_bytes: 128}
    assert {[], state} = poll(state)
    assert {[], _state} = poll(state)
    # The cursor also preserves discarding mode across a restart.
    {:producer, state} =
      Producer.init(path: path, cursor_path: cursor, max_bytes: 64, max_line_bytes: 128)

    assert {[], state} = poll(state)
    assert {[], state} = poll(state)
    assert {[%{data: %{"request_id" => "good"}}], _} = poll(state)
  end

  test "draining stops new emissions", %{path: path, state: state} do
    File.write!(path, line("one"))
    {:noreply, [], state} = Producer.prepare_for_draining(state)
    assert {[], _} = poll(state)
  end
end
