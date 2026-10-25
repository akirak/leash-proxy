defmodule LeashCollector.Audit do
  @moduledoc """
  Audit event context: the query API powering the monitoring UI (ADR 0002/0003).

  Reading side (this module) is completely decoupled from the ingestion
  side (`LeashCollector.JsonlTailProducer` → `LeashCollector.AuditPipeline`
  → `LeashCollector.Audit.Store`), so the transport can graduate from file
  tailing to a message broker without touching the UI.
  """

  import Ecto.Query

  alias LeashCollector.Audit.{Event, Store}
  alias LeashCollector.Repo

  @decisions ~w(allow deny)
  @windows %{
    "15m" => 15,
    "1h" => 1,
    "6h" => 6,
    "24h" => 24,
    "7d" => 7 * 24
  }

  defdelegate insert_batch(records), to: Store
  defdelegate prune_before(cutoff), to: Store
  defdelegate subscribe, to: Store

  def decisions, do: @decisions
  def windows, do: Map.keys(@windows)

  @doc "Fetches a single event by its request id."
  def get_event(request_id) when is_binary(request_id) do
    Repo.get_by(Event, request_id: request_id)
  end

  @doc """
  Lists events, newest first, with filtering and pagination.

  Supported filters:

    * `:decision` - `"allow"` or `"deny"`
    * `:agent_id` - agent id substring
    * `:host` - host substring
    * `:path` - path substring
    * `:since` - one of `windows/0` or `:all`
    * `:page` - 1-based page number
  """
  def list_events(filters \\ []) do
    per_page = Keyword.get(filters, :per_page, 50)
    page = max(Keyword.get(filters, :page, 1) || 1, 1)

    query =
      Event
      |> filter_query(filters)
      |> order_by(desc: :ts, desc: :id)

    total = Repo.aggregate(query, :count)

    events =
      Repo.all(
        from(e in query,
          limit: ^per_page,
          offset: ^((page - 1) * per_page)
        )
      )

    %{events: events, total: total, page: page, per_page: per_page}
  end

  defp filter_query(query, filters) do
    Enum.reduce(filters, query, fn
      {:decision, decision}, query when decision in @decisions ->
        where(query, [e], e.decision == ^decision)

      {:agent_id, agent}, query when is_binary(agent) and agent != "" ->
        where(query, [e], like(e.agent_id, ^"%#{escape(agent)}%"))

      {:host, host}, query when is_binary(host) and host != "" ->
        where(query, [e], like(e.host, ^"%#{escape(host)}%"))

      {:path, path}, query when is_binary(path) and path != "" ->
        where(query, [e], like(e.path, ^"%#{escape(path)}%"))

      {:since, window}, query when window != "all" and is_map_key(@windows, window) ->
        hours = Map.fetch!(@windows, window)
        where(query, [e], e.ts >= ^ago(hours))

      _, query ->
        query
    end)
  end

  defp ago(hours) do
    DateTime.utc_now() |> DateTime.add(-hours, :hour) |> DateTime.truncate(:second)
  end

  defp escape(value), do: String.replace(value, "%", "\\%")

  @doc """
  Aggregated stats: event counts by decision and top denied hosts.

  Accepts the same `:since` filter as `list_events/1`.
  """
  def stats(filters \\ []) do
    since = Keyword.get(filters, :since, "24h")

    counts =
      Repo.all(
        from(e in Event,
          select: {e.decision, count(e.id)},
          where: e.ts >= ^window_cutoff(since),
          group_by: e.decision
        )
      )
      |> Map.new()

    top_denied_hosts =
      Repo.all(
        from(e in Event,
          select: %{host: e.host, count: count(e.id)},
          where: e.decision == "deny" and e.ts >= ^window_cutoff(since),
          group_by: e.host,
          order_by: [desc: count(e.id)],
          limit: 10
        )
      )

    total = Map.get(counts, "allow", 0) + Map.get(counts, "deny", 0)

    %{
      total: total,
      allow: Map.get(counts, "allow", 0),
      deny: Map.get(counts, "deny", 0),
      top_denied_hosts: top_denied_hosts
    }
  end

  defp window_cutoff("all"), do: ~U[1970-01-01 00:00:00Z]

  defp window_cutoff(window) when is_map_key(@windows, window) do
    ago(Map.fetch!(@windows, window))
  end

  # Unknown/blank windows (e.g. a stale form value) fall back to 24h.
  defp window_cutoff(_other), do: ago(Map.fetch!(@windows, "24h"))
end
