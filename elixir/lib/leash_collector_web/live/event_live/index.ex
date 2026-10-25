defmodule LeashCollectorWeb.EventLive.Index do
  @moduledoc """
  Live-updating browser of indexed proxy audit events with filtering,
  pagination, and drill-down into individual requests.
  """
  use LeashCollectorWeb, :live_view
  alias LeashCollector.Audit
  @per_page 50
  def mount(_params, _session, socket) do
    Audit.subscribe()
    {:ok, init(socket)}
  end

  defp init(socket) do
    socket
    |> assign(:live_updates, true)
    |> assign(:pending, 0)
    |> assign(:page, 1)
    |> assign(:filters, %{
      "decision" => "",
      "agent_id" => "",
      "host" => "",
      "path" => "",
      "since" => "1h"
    })
    |> refresh()
  end

  def handle_params(_params, _uri, socket), do: {:noreply, socket}

  def handle_event("filter", params, socket) do
    filters = merge_filters(socket.assigns.filters, params)

    socket
    |> assign(:filters, filters)
    |> assign(:page, 1)
    |> refresh()
    |> noreply()
  end

  def handle_event("reset", _params, socket) do
    socket
    |> assign(:page, 1)
    |> assign(:filters, %{
      "decision" => "",
      "agent_id" => "",
      "host" => "",
      "path" => "",
      "since" => "1h"
    })
    |> refresh()
    |> noreply()
  end

  def handle_event("toggle-live", _params, socket) do
    {:noreply, assign(socket, live_updates: !socket.assigns.live_updates, pending: 0)}
  end

  def handle_event("page", %{"page" => page}, socket) do
    socket
    |> assign(:page, max(String.to_integer(page), 1))
    |> refresh()
    |> noreply()
  end

  def handle_info({:audit_events_inserted, count}, socket) do
    if socket.assigns.live_updates do
      # Debounce refreshes so bursts of batches trigger one re-query.
      pending = socket.assigns.pending + count

      if socket.assigns[:refresh_timer] do
        {:noreply, assign(socket, :pending, pending)}
      else
        {:noreply,
         socket
         |> assign(:pending, pending)
         |> schedule_refresh()}
      end
    else
      {:noreply, socket}
    end
  end

  def handle_info(:refresh, socket) do
    {:noreply,
     socket
     |> assign(:refresh_timer, nil)
     |> assign(:pending, 0)
     |> refresh()}
  end

  def handle_info(_other, socket), do: {:noreply, socket}

  defp refresh(socket) do
    assigns = socket.assigns

    filters = [
      decision: assigns.filters["decision"],
      agent_id: assigns.filters["agent_id"],
      host: assigns.filters["host"],
      path: assigns.filters["path"],
      since: assigns.filters["since"],
      page: assigns.page,
      per_page: @per_page
    ]

    result = Audit.list_events(filters)
    stats = Audit.stats(since: assigns.filters["since"])

    socket
    |> assign(:result, result)
    |> assign(:stats, stats)
    |> assign(:total_pages, max(ceil(result.total / @per_page), 1))
  end

  defp merge_filters(current, params) do
    params =
      Map.new(current, fn {key, _value} ->
        {key, normalize_value(params[key])}
      end)

    Map.update!(params, "since", fn window ->
      if window == "all" or window in Audit.windows(), do: window, else: "1h"
    end)
  end

  defp normalize_value(value) when is_binary(value), do: String.trim(value)
  defp normalize_value(_other), do: ""

  defp schedule_refresh(socket) do
    timer = Process.send_after(self(), :refresh, 250)
    assign(socket, :refresh_timer, timer)
  end

  defp noreply(socket), do: {:noreply, socket}

  def render(assigns) do
    ~H"""
    <Layouts.app flash={@flash}>
      <.header>
        Audit events
        <:subtitle>
          Indexed requests from the leash-proxy audit trail (JSONL is the source of truth; this index is rebuildable).
        </:subtitle>
        <:actions>
          <div class="flex items-center gap-2">
            <span :if={@pending > 0} class="text-sm text-base-content/70">
              +{@pending} new
            </span>
            <.button phx-click="toggle-live">
              <span :if={@live_updates} class="flex items-center gap-1">
                <span class="relative flex size-2">
                  <span class="animate-ping absolute inline-flex size-full rounded-full bg-success opacity-75"></span>
                  <span class="relative inline-flex size-2 rounded-full bg-success"></span>
                </span>
                Live
              </span>
              <span :if={not @live_updates}>Paused</span>
            </.button>
          </div>
        </:actions>
      </.header>
      <form phx-change="filter" class="mt-4 grid gap-2 sm:grid-cols-6">
        <label class="input input-bordered flex items-center gap-2">
          <span class="text-xs text-base-content/60">Decision</span>
          <select class="grow" name="decision">
            <option value="">any</option>
            <option
              :for={decision <- Audit.decisions()}
              value={decision}
              selected={@filters["decision"] == decision}
            >
              {decision}
            </option>
          </select>
        </label>
        <label class="input input-bordered flex items-center gap-2">
          <span class="text-xs text-base-content/60">Agent</span>
          <input
            class="grow"
            type="text"
            name="agent_id"
            value={@filters["agent_id"]}
            placeholder="agent id"
          />
        </label>
        <label class="input input-bordered flex items-center gap-2">
          <span class="text-xs text-base-content/60">Host</span>
          <input
            class="grow"
            type="text"
            name="host"
            value={@filters["host"]}
            placeholder="e.g. github.com"
          />
        </label>
        <label class="input input-bordered flex items-center gap-2">
          <span class="text-xs text-base-content/60">Path</span>
          <input class="grow" type="text" name="path" value={@filters["path"]} placeholder="/api" />
        </label>
        <label class="input input-bordered flex items-center gap-2">
          <span class="text-xs text-base-content/60">Window</span>
          <select class="grow" name="since">
            <option
              :for={window <- Audit.windows()}
              value={window}
              selected={@filters["since"] == window}
            >
              last {window}
            </option>
            <option value="all" selected={@filters["since"] == "all"}>all time</option>
          </select>
        </label>
        <button type="button" phx-click="reset" class="btn btn-ghost">Reset</button>
      </form>
      <div class="mt-4 flex flex-wrap gap-4 text-sm text-base-content/70">
        <span>{@result.total} matching events</span>
        <span class="text-success">{@stats.allow} allowed</span>
        <span class="text-error">{@stats.deny} denied</span>
      </div>
      <div class="mt-2 overflow-x-auto">
        <.table
          id="events"
          rows={@result.events}
          row_id={&"event-#{&1.request_id}"}
          row_click={&JS.navigate(~p"/events/#{&1.request_id}")}
        >
          <:col :let={event} label="Timestamp">
            <code class="text-xs">{format_ts(event.ts)}</code>
          </:col>
          <:col :let={event} label="Decision"><.decision_badge decision={event.decision} /></:col>
          <:col :let={event} label="Agent"><code class="text-xs">{event.agent_id}</code></:col>
          <:col :let={event} label="Method">{event.method}</:col>
          <:col :let={event} label="Host">{event.host}</:col>
          <:col :let={event} label="Path">
            <span class="block max-w-96 truncate" title={event.path}>{event.path}</span>
          </:col>
          <:col :let={event} label="Status">{event.status}</:col>
          <:col :let={event} label="Latency">{format_latency(event.latency_ms)}</:col>
          <:col :let={event} label="In/Out">
            {format_bytes(event.bytes_in)}/{format_bytes(event.bytes_out)}
          </:col>
        </.table>
      </div>
      <div class="mt-6 flex items-center justify-between">
        <button
          class="btn btn-sm btn-ghost"
          phx-click="page"
          phx-value-page={@page - 1}
          disabled={@page <= 1}
        >
          ← Newer
        </button>
        <span>Page {@page} of {@total_pages}</span>
        <button
          class="btn btn-sm btn-ghost"
          phx-click="page"
          phx-value-page={@page + 1}
          disabled={@page >= @total_pages}
        >
          Older →
        </button>
      </div>
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
    Calendar.strftime(ts, "%Y-%m-%d %H:%M:%SZ")
  end

  defp format_latency(ms) when is_number(ms) and ms >= 1000, do: "#{Float.round(ms / 1000, 1)}s"
  defp format_latency(ms) when is_number(ms), do: "#{Float.round(ms, 1)}ms"
  defp format_bytes(bytes) when is_integer(bytes) and bytes >= 1024, do: "#{div(bytes, 1024)}KiB"
  defp format_bytes(bytes) when is_integer(bytes), do: "#{bytes}B"
end
