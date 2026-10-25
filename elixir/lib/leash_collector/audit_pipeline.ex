defmodule LeashCollector.AuditPipeline do
  @moduledoc """
  The Broadway ingestion pipeline (ADR 0003): leash-proxy JSONL audit events
  are tailed from disk, batched, and inserted into SQLite.

  ```text
  JSONL audit log (source of truth)
    └─ JsonlTailProducer ─► Broadway processors ─► :audit_events batcher
                                                      └─ Audit.Store.insert_batch/1 (SQLite)
                                                           └─ PubSub "audit_events" ─► LiveViews
  ```

  The producer is transport-specific (file tailing today, a message broker
  later); everything downstream of it is transport-agnostic. To use a broker
  instead, replace the producer module below with a broker producer
  (e.g. `OffBroadway.Kafka.Producer`) and keep this module unchanged.
  """

  use Broadway

  require Logger

  alias Broadway.Message
  alias LeashCollector.Audit
  alias LeashCollector.Audit.Record

  def start_link(opts \\ []) do
    opts = Keyword.merge(default_opts(), opts)
    Broadway.start_link(__MODULE__, opts)
  end

  defp default_opts do
    [
      name: __MODULE__,
      producer: [
        module: {LeashCollector.JsonlTailProducer, producer_opts()}
      ],
      processors: [
        default: [concurrency: 2]
      ],
      batchers: [
        audit_events: [concurrency: 1, batch_size: 200, batch_timeout: 1_000]
      ]
    ]
  end

  defp producer_opts do
    [
      path: Application.get_env(:leash_collector, :audit_log_path, "tmp/audit.jsonl"),
      cursor_path: Application.get_env(:leash_collector, :audit_cursor_path),
      poll_interval: Application.get_env(:leash_collector, :audit_poll_interval, 500)
    ]
  end

  @impl true
  def handle_message(_processor, %Message{} = message, _context) do
    message = Message.put_batcher(message, :audit_events)

    case Record.from_json(message.data) do
      {:ok, params} ->
        Message.put_data(message, params)

      {:error, reason} = error ->
        # The producer filters non-audit lines, so only malformed audit
        # records land here. Discard them but keep the reason visible.
        Logger.warning("leash collector: discarding audit event: #{inspect(reason)}")
        Message.put_data(message, {:discarded, error})
    end
  end

  @impl true
  def handle_batch(:audit_events, messages, _batch_info, _context) do
    records = for message <- messages, is_map(message.data), do: message.data

    count = Audit.insert_batch(records)
    if count > 0, do: Logger.info("leash collector: indexed #{count} audit events")

    # Broadway requires the batched messages to be returned.
    messages
  end
end
