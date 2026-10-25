defmodule LeashCollector.JsonlTailProducer do
  @moduledoc """
  A Broadway producer that tails the leash-proxy append-only JSONL audit log
  (ADR 0003: event transport for a same-host deployment).

  One `Broadway.Message` is emitted per audit event line. The byte offset and
  inode of the tailed file are persisted to a cursor file so ingestion
  resumes where it left off after restarts; when the cursor no longer
  matches the file (rotation, truncation), the file is replayed from byte 0.
  The JSONL log remains the source of truth (ADR 0002).

  * Non-audit log lines (different `target`) are skipped without indexing.
  * Only complete newline-terminated lines are consumed, so a partially
    flushed trailing line is picked up on the next poll.
  * Backpressure: at most one bounded chunk is read per poll and never more
    than the outstanding GenStage demand is emitted, so the proxy's hot path
    (appending JSONL) is never blocked.
  * Idempotency downstream (`on_conflict: :nothing` on `request_id`) makes
    at-least-once re-emission after a crash safe.

  When events are eventually exported over the network, this module can be
  swapped for a broker-backed Broadway producer (e.g. Broadway Kafka, SQS,
  or Redis) without any other pipeline changes.
  """

  use GenStage

  @behaviour Broadway.Producer

  require Logger

  alias Broadway.Message
  alias Broadway.NoopAcknowledger
  alias LeashCollector.Audit.Record

  @default_opts [
    poll_interval: 500,
    max_bytes: 512_000
  ]

  @doc """
  Starts the producer standalone (outside a Broadway topology).

  Used only for tests and manual experimentation; Broadway starts it via
  `init/1` with the same options.
  """
  def start_link(opts) do
    GenStage.start_link(__MODULE__, Keyword.merge(@default_opts, opts))
  end

  @impl true
  def init(opts) do
    path = Keyword.fetch!(opts, :path)
    cursor_path = Keyword.get(opts, :cursor_path) || default_cursor_path(path)
    poll_interval = Keyword.get(opts, :poll_interval, 500)
    max_bytes = Keyword.get(opts, :max_bytes, 512_000)

    state = %{
      path: path,
      cursor_path: cursor_path,
      poll_interval: poll_interval,
      max_bytes: max_bytes,
      demand: 0,
      offset: 0,
      inode: nil
    }

    state = resume(state)
    Process.send_after(self(), :poll, 0)

    {:producer, state}
  end

  @impl GenStage
  def handle_demand(demand, state) when demand > 0 do
    # Poll eagerly on new demand for lower ingest latency when idle.
    send(self(), :poll)
    {:noreply, [], %{state | demand: state.demand + demand}}
  end

  def handle_demand(_demand, state), do: {:noreply, [], state}

  @impl GenStage
  def handle_info(:poll, state) do
    {events, offset, inode, backlog?} = read_chunk(state)

    state =
      %{state | demand: max(state.demand - length(events), 0), offset: offset, inode: inode}
      |> flush_cursor()

    delay = if backlog? and state.demand > 0, do: 10, else: state.poll_interval
    Process.send_after(self(), :poll, delay)

    {:noreply, events, state}
  end

  def handle_info(_other, state), do: {:noreply, [], state}

  @impl GenStage
  def terminate(_reason, state), do: flush_cursor(state)

  @impl Broadway.Producer
  def prepare_for_draining(_state), do: :ok

  ## Polling

  defp read_chunk(state) when state.demand <= 0, do: {[], state.offset, state.inode, false}

  defp read_chunk(state) do
    case File.stat(state.path) do
      {:ok, stat} ->
        offset = start_offset(state, stat)

        {events, consumed, backlog?} =
          scan(state.path, offset, state.demand, state.max_bytes)

        {events, offset + consumed, stat.inode, backlog?}

      {:error, reason} ->
        # The log file does not exist yet (e.g. the proxy has not started).
        debug("cannot stat #{state.path}: #{inspect(reason)}")
        {[], state.offset, state.inode, false}
    end
  end

  defp resume(state) do
    with {:ok, stat} <- File.stat(state.path),
         {:ok, offset, inode} <- read_cursor(state),
         true <- inode == stat.inode and stat.size >= offset do
      %{state | offset: offset, inode: inode}
    else
      _ -> state
    end
  end

  # Cursor sanity: resume only when the same file still contains the cursor
  # offset; anything else (rotation, truncation, stale cursor) replays from 0.
  defp start_offset(state, stat) do
    cond do
      state.inode == nil -> resume_offset(state, stat)
      state.inode == stat.inode and stat.size >= state.offset -> state.offset
      true -> 0
    end
  end

  defp resume_offset(state, stat) do
    case read_cursor(state) do
      {:ok, offset, inode} when inode == stat.inode and stat.size >= offset -> offset
      _ -> 0
    end
  end

  defp scan(path, offset, demand, max_bytes) do
    case :file.open(path, [:read, :binary]) do
      {:ok, device} ->
        result =
          with {:ok, _position} <- :file.position(device, offset),
               {:ok, data} <- :file.read(device, max_bytes) do
            {:ok, data}
          end

        :file.close(device)

        case result do
          {:ok, data} ->
            scan_chunk(data, demand)

          :eof ->
            # Fully consumed; nothing to index this poll.
            {[], 0, false}

          other ->
            debug("cannot read #{path} at offset #{offset}: #{inspect(other)}")
            {[], 0, false}
        end

      {:error, reason} ->
        debug("cannot open #{path}: #{inspect(reason)}")
        {[], 0, false}
    end
  end

  # Consume only complete (newline-terminated) lines, at most `demand`
  # audit events, and stop exactly after the last examined line so nothing
  # is lost between polls. A trailing partial line (the proxy may still be
  # flushing it) is left for a later poll — this also guarantees the
  # consumed offset never runs past the end of the file.
  defp scan_chunk(data, demand) do
    case :binary.matches(data, "\n") do
      [] ->
        {[], 0, false}

      matches ->
        {last_newline_at, 1} = List.last(matches)
        complete = binary_part(data, 0, last_newline_at + 1)

        complete
        |> String.split("\n", trim: true)
        |> scan_lines(demand, [], 0, 0)
    end
  end

  defp scan_lines([], _demand, events, consumed, _backlog),
    do: {Enum.reverse(events), consumed, false}

  defp scan_lines(_rest, 0, events, consumed, _backlog),
    do: {Enum.reverse(events), consumed, true}

  defp scan_lines([line | rest], demand, events, consumed, emitted) do
    line_bytes = byte_size(line) + 1

    case decode(line) do
      {:ok, json} when emitted < demand ->
        message = to_message(json, consumed)
        scan_lines(rest, demand, [message | events], consumed + line_bytes, emitted + 1)

      {:ok, _json} ->
        # Demand exhausted; rewind to before this line (re-read next poll).
        {Enum.reverse(events), consumed, true}

      {:skip, reason} ->
        debug("skipped non-indexable line: #{reason}")
        scan_lines(rest, demand, events, consumed + line_bytes, emitted)

      {:error, reason} ->
        # Undecodable line; log and consume it so a single bad line cannot
        # wedge the tail loop forever.
        Logger.warning("leash collector: malformed JSONL line: #{reason}")
        scan_lines(rest, demand, events, consumed + line_bytes, emitted)
    end
  end

  defp decode("") do
    {:skip, "empty line"}
  end

  defp decode(line) do
    case Jason.decode(line) do
      {:ok, json} when is_map(json) ->
        if Record.audit?(json) do
          {:ok, json}
        else
          {:skip, "target=#{json["target"] || "none"}"}
        end

      {:ok, _other} ->
        {:skip, "not an object"}

      {:error, %Jason.DecodeError{} = error} ->
        {:error, Exception.message(error)}
    end
  end

  defp to_message(json, offset) do
    %Message{
      data: json,
      acknowledger: NoopAcknowledger.init(),
      metadata: %{"offset" => offset}
    }
  end

  ## Cursor persistence

  defp default_cursor_path(path) do
    dir = Path.dirname(path)
    Path.join(dir, ".#{Path.basename(path)}.cursor")
  end

  defp read_cursor(state) do
    with {:ok, raw} <- File.read(state.cursor_path),
         {:ok, %{"offset" => offset, "inode" => inode}}
         when is_integer(offset) and is_integer(inode) <-
           Jason.decode(raw) do
      {:ok, offset, inode}
    else
      _ -> :error
    end
  end

  defp flush_cursor(%{offset: 0, inode: nil} = state), do: state

  defp flush_cursor(state) do
    content = Jason.encode!(%{"offset" => state.offset, "inode" => state.inode})

    case File.write(state.cursor_path, content <> "\n") do
      :ok ->
        state

      {:error, reason} ->
        debug("cannot write cursor #{state.cursor_path}: #{inspect(reason)}")
        state
    end
  end

  defp debug(message), do: Logger.debug("leash collector: #{message}")
end
