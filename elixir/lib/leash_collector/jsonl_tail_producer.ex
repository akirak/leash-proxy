defmodule LeashCollector.JsonlTailProducer do
  @moduledoc """
  Demand-driven Broadway producer for the proxy's append-only JSONL log.

  Only one bounded chunk is in flight at a time. Its cursor is atomically
  checkpointed only after every emitted message is acknowledged. Failed
  chunks are replayed; SQLite's unique request IDs make replays idempotent.
  Incomplete lines are left for the next poll. Keep rotated logs until drained;
  rotation or detected truncation starts the current file at byte zero.
  """
  use GenStage
  @behaviour Broadway.Producer
  @behaviour Broadway.Acknowledger

  require Logger
  alias Broadway.Message
  alias LeashCollector.Audit.Record

  def start_link(opts), do: GenStage.start_link(__MODULE__, opts)

  @impl GenStage
  def init(opts) do
    path = Keyword.fetch!(opts, :path)

    state = %{
      path: path,
      cursor_path:
        Keyword.get(opts, :cursor_path) ||
          Path.join(Path.dirname(path), ".#{Path.basename(path)}.cursor"),
      poll_interval: Keyword.get(opts, :poll_interval, 500),
      max_bytes: Keyword.get(opts, :max_bytes, 512_000),
      read_bytes: Keyword.get(opts, :max_bytes, 512_000),
      max_line_bytes: Keyword.get(opts, :max_line_bytes, 16_777_216),
      discarding: false,
      demand: 0,
      offset: 0,
      inode: nil,
      pending: nil,
      timer: nil,
      draining: false
    }

    {:producer, schedule(resume(state), 0)}
  end

  @impl GenStage
  def handle_demand(demand, state) do
    {:noreply, [], schedule(%{state | demand: state.demand + demand}, 0)}
  end

  @impl GenStage
  def handle_info(:poll, state) do
    state = %{state | timer: nil}

    if state.draining or state.demand <= 0 or state.pending != nil do
      {:noreply, [], state}
    else
      {events, state} = poll(state)
      {:noreply, events, schedule(state, state.poll_interval)}
    end
  end

  def handle_info(
        {:ack, token, successful, failed},
        %{pending: %{token: token} = pending} = state
      ) do
    pending = %{
      pending
      | remaining: pending.remaining - successful - failed,
        failed: pending.failed or failed > 0
    }

    state = %{state | pending: pending}

    state =
      if pending.remaining == 0 do
        state = %{state | pending: nil}

        if pending.failed do
          # Leave the durable cursor unchanged and re-read the entire chunk.
          state
        else
          checkpoint(%{state | offset: pending.offset, inode: pending.inode})
        end
      else
        state
      end

    {:noreply, [], schedule(state, state.poll_interval)}
  end

  def handle_info(_message, state), do: {:noreply, [], state}

  @impl Broadway.Producer
  def prepare_for_draining(state), do: {:noreply, [], %{state | draining: true}}

  @impl Broadway.Acknowledger
  def ack({producer, token}, successful, failed) do
    send(producer, {:ack, token, length(successful), length(failed)})
    :ok
  end

  defp schedule(%{timer: nil, draining: false} = state, delay) do
    %{state | timer: Process.send_after(self(), :poll, delay)}
  end

  defp schedule(state, _delay), do: state

  defp poll(state) do
    with {:ok, device} <- :file.open(state.path, [:read, :binary, :raw]) do
      try do
        with {:ok, info} <- :file.read_file_info(device) do
          stat = File.Stat.from_record(info)
          same_file = stat.inode == state.inode and stat.size >= state.offset
          offset = if same_file, do: state.offset, else: 0

          state =
            if same_file,
              do: state,
              else: %{state | discarding: false, read_bytes: state.max_bytes}

          case :file.pread(device, offset, state.read_bytes) do
            {:ok, data} ->
              {records, consumed, state} = scan_window(data, state)
              end_offset = offset + consumed
              token = make_ref()

              events =
                Enum.map(records, fn {json, relative_offset} ->
                  %Message{
                    data: json,
                    acknowledger: {__MODULE__, {self(), token}, nil},
                    metadata: %{offset: offset + relative_offset}
                  }
                end)

              if events == [] do
                next = %{state | offset: end_offset, inode: stat.inode}
                {[], if(consumed > 0, do: checkpoint(next), else: next)}
              else
                pending = %{
                  token: token,
                  remaining: length(events),
                  failed: false,
                  offset: end_offset,
                  inode: stat.inode
                }

                {events, %{state | demand: state.demand - length(events), pending: pending}}
              end

            _ ->
              {[], state}
          end
        else
          _ -> {[], state}
        end
      after
        :file.close(device)
      end
    else
      _ -> {[], state}
    end
  end

  # Grow the read window for a line spanning chunks, but never retain an
  # unbounded line. Oversized lines are discarded through their next newline.
  defp scan_window(data, %{discarding: true} = state) do
    case :binary.match(data, "\n") do
      :nomatch -> {[], byte_size(data), state}
      {at, 1} -> {[], at + 1, %{state | discarding: false, read_bytes: state.max_bytes}}
    end
  end

  defp scan_window(data, state) do
    {records, consumed} = scan(data, state.demand, [], 0)

    cond do
      consumed > 0 ->
        {records, consumed, %{state | read_bytes: state.max_bytes}}

      byte_size(data) < state.read_bytes ->
        {[], 0, state}

      state.read_bytes < state.max_line_bytes ->
        {[], 0, %{state | read_bytes: min(state.read_bytes * 2, state.max_line_bytes)}}

      true ->
        Logger.warning(
          "leash collector: discarding JSONL line exceeding #{state.max_line_bytes} bytes"
        )

        {[], byte_size(data), %{state | discarding: true, read_bytes: state.max_bytes}}
    end
  end

  defp scan(_data, 0, events, consumed), do: {Enum.reverse(events), consumed}

  defp scan(data, demand, events, consumed) do
    case :binary.match(data, "\n") do
      :nomatch ->
        {Enum.reverse(events), consumed}

      {at, 1} ->
        line = binary_part(data, 0, at)
        rest = binary_part(data, at + 1, byte_size(data) - at - 1)

        case Jason.decode(line) do
          {:ok, json} ->
            if Record.audit?(json) do
              scan(rest, demand - 1, [{json, consumed} | events], consumed + at + 1)
            else
              scan(rest, demand, events, consumed + at + 1)
            end

          {:error, _} ->
            if line != "", do: Logger.warning("leash collector: skipping malformed JSONL line")
            scan(rest, demand, events, consumed + at + 1)
        end
    end
  end

  defp resume(state) do
    with {:ok, raw} <- File.read(state.cursor_path),
         {:ok, %{"offset" => offset, "inode" => inode}}
         when is_integer(offset) and offset >= 0 and is_integer(inode) <- Jason.decode(raw),
         {:ok, stat} <- File.stat(state.path),
         true <- inode == stat.inode and stat.size >= offset do
      cursor = Jason.decode!(raw)
      %{state | offset: offset, inode: inode, discarding: cursor["discarding"] == true}
    else
      _ -> state
    end
  end

  defp checkpoint(%{inode: nil} = state), do: state

  defp checkpoint(state) do
    content =
      Jason.encode!(%{offset: state.offset, inode: state.inode, discarding: state.discarding}) <>
        "\n"

    temporary = state.cursor_path <> ".tmp"

    with :ok <- File.mkdir_p(Path.dirname(state.cursor_path)),
         :ok <- File.write(temporary, content, [:sync]),
         :ok <- File.rename(temporary, state.cursor_path) do
      state
    else
      {:error, reason} ->
        Logger.warning("leash collector: cannot checkpoint cursor: #{inspect(reason)}")
        state
    end
  end
end
