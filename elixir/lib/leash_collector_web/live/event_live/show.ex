defmodule LeashCollectorWeb.EventLive.Show do
  @moduledoc """
  Drill-down into a single indexed proxy audit event.
  """

  use LeashCollectorWeb, :live_view

  alias LeashCollector.Audit

  def mount(_params, _session, socket) do
    {:ok, assign(socket, :event, nil)}
  end

  def handle_params(%{"request_id" => request_id}, _uri, socket) do
    case Audit.get_event(request_id) do
      nil ->
        socket
        |> put_flash(:error, "No indexed event with request_id #{request_id}")
        |> push_navigate(to: ~p"/events")
        |> noreply()

      event ->
        {:noreply, assign(socket, event: event)}
    end
  end

  def handle_info(_other, socket), do: {:noreply, socket}

  def render(assigns) do
    ~H"""
    <Layouts.app flash={@flash}>
      <.header>
        <span class="flex items-center gap-2">
          Audit event <.decision_badge decision={@event.decision} />
        </span>
        <:subtitle>
          Request <code>{@event.request_id}</code> from agent <code>{@event.agent_id}</code>
        </:subtitle>
        <:actions>
          <.button navigate={~p"/events"}>Back to events</.button>
        </:actions>
      </.header>

      <.list>
        <:item title="Timestamp">{format_ts(@event.ts)}</:item>
        <:item title="Decision">
          {@event.decision}
          <span :if={@event.decision == "deny"} class="text-sm text-base-content/70">
            {@event.deny_reason}
          </span>
        </:item>
        <:item title="Policy engine">{@event.engine}</:item>
        <:item title="Determining policies">{@event.policy_ids}</:item>
        <:item title="Source">{@event.source_ip}</:item>
        <:item title="Target">
          {@event.scheme}://{@event.host}:{@event.port}{@event.path}
        </:item>
        <:item title="Method">{@event.method}</:item>
        <:item title="Status">{@event.status}</:item>
        <:item title="Latency">{format_latency(@event.latency_ms)}</:item>
        <:item title="Bytes in / out">
          {format_bytes(@event.bytes_in)} / {format_bytes(@event.bytes_out)}
        </:item>
        <:item title="TLS">{@event.tls}</:item>
        <:item :if={@event.error != ""} title="Transport error">{@event.error}</:item>
        <:item title="Recorded headers (proxy allowlist, redacted)">
          <pre class="mt-1 max-w-xl overflow-x-auto rounded bg-base-200 p-2 text-xs">{@event.headers}</pre>
        </:item>
      </.list>
    </Layouts.app>
    """
  end

  attr :decision, :string, required: true

  defp decision_badge(%{decision: "allow"} = assigns) do
    ~H"""
    <span class="badge badge-success badge-sm">{@decision}</span>
    """
  end

  defp decision_badge(%{decision: "deny"} = assigns) do
    ~H"""
    <span class="badge badge-error badge-sm">{@decision}</span>
    """
  end

  defp decision_badge(assigns) do
    ~H"""
    <span class="badge badge-ghost badge-sm">{@decision}</span>
    """
  end

  defp format_ts(%DateTime{} = ts) do
    Calendar.strftime(ts, "%Y-%m-%d %H:%M:%S.%fZ")
  end

  defp format_latency(ms) when is_number(ms) and ms >= 1000, do: "#{Float.round(ms / 1000, 1)}s"
  defp format_latency(ms) when is_number(ms), do: "#{Float.round(ms, 1)}ms"

  defp format_bytes(bytes) when is_integer(bytes) and bytes >= 1024, do: "#{div(bytes, 1024)}KiB"
  defp format_bytes(bytes) when is_integer(bytes), do: "#{bytes}B"

  defp noreply(socket), do: {:noreply, socket}
end
