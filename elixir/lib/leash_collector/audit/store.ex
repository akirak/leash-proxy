defmodule LeashCollector.Audit.Store do
  @moduledoc """
  The Broadway batch sink: idempotent batch inserts into SQLite and
  PubSub notification for live-updating LiveViews.
  """

  import Ecto.Query

  alias LeashCollector.Audit.Event
  alias LeashCollector.Repo

  @pubsub LeashCollector.PubSub
  @topic "audit_events"

  @doc """
  Inserts a batch of normalized `Event` params and broadcasts a notification.

  Returns the number of newly inserted rows (conflicting rows are skipped).
  Idempotent: rows conflicting on `request_id` are skipped (`ON CONFLICT DO
  NOTHING`), so replays from the JSONL log (or a future message broker with
  at-least-once delivery) cannot duplicate events.
  """
  @spec insert_batch([map()]) :: non_neg_integer()
  def insert_batch([]), do: 0

  def insert_batch(records) when is_list(records) do
    inserted_at = DateTime.utc_now() |> DateTime.truncate(:second)

    rows =
      Enum.map(records, fn record ->
        record
        |> Map.put(:inserted_at, inserted_at)
        |> Map.take(Event.__schema__(:fields))
      end)

    {count, _} =
      Repo.insert_all(Event, rows,
        on_conflict: :nothing,
        conflict_target: :request_id
      )

    if count > 0, do: broadcast(count)
    count
  end

  @doc """
  Broadcasts that new events became visible to the UI.
  """
  def broadcast(count) do
    Phoenix.PubSub.broadcast(@pubsub, @topic, {:audit_events_inserted, count})
  end

  @doc "Subscribes the calling process to batch notifications."
  def subscribe do
    Phoenix.PubSub.subscribe(@pubsub, @topic)
  end

  @doc "Prunes events older than the given `DateTime` (retention, ADR 0002)."
  def prune_before(%DateTime{} = cutoff) do
    query = from(e in Event, where: e.ts < ^cutoff)
    {count, _} = Repo.delete_all(query)
    count
  end
end
