defmodule LeashCollectorWeb.EventLive.Index do
  @moduledoc "Live audit browser with URL-backed filters, pagination and bounded streams."
  use LeashCollectorWeb, :live_view
  alias LeashCollector.Audit

  @per_page 50
  @defaults %{"decision" => "", "agent_id" => "", "host" => "", "path" => "", "since" => "1h"}
  @input_class "w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 focus:border-teal-600 focus:outline-none focus:ring-2 focus:ring-teal-100"

  def mount(_params, _session, socket) do
    if connected?(socket), do: Audit.subscribe()
    {:ok, assign(socket, live_updates: true, pending: 0, refresh_timer: nil)}
  end

  def handle_params(params, _uri, socket) do
    filters = normalize_filters(Map.merge(@defaults, params))
    {:noreply, socket |> assign(filters: filters, page: page(params["page"])) |> refresh()}
  end

  def handle_event("filter", params, socket) do
    filters = normalize_filters(Map.merge(socket.assigns.filters, params))
    {:noreply, push_patch(socket, to: ~p"/events?#{filters}")}
  end

  def handle_event("reset", _params, socket), do: {:noreply, push_patch(socket, to: ~p"/events")}

  def handle_event("page", %{"page" => value}, socket) do
    params = Map.put(socket.assigns.filters, "page", min(page(value), socket.assigns.total_pages))
    {:noreply, push_patch(socket, to: ~p"/events?#{params}")}
  end

  def handle_event("toggle-live", _params, socket) do
    socket = assign(socket, :live_updates, not socket.assigns.live_updates)
    {:noreply, if(socket.assigns.live_updates, do: refresh(socket), else: socket)}
  end

  def handle_info({:audit_events_inserted, count}, socket) do
    socket = assign(socket, :pending, socket.assigns.pending + count)

    socket =
      if socket.assigns.live_updates and is_nil(socket.assigns.refresh_timer) do
        assign(socket, :refresh_timer, Process.send_after(self(), :refresh, 250))
      else
        socket
      end

    {:noreply, socket}
  end

  def handle_info(:refresh, socket) do
    socket = assign(socket, :refresh_timer, nil)
    {:noreply, if(socket.assigns.live_updates, do: refresh(socket), else: socket)}
  end

  defp refresh(socket) do
    f = socket.assigns.filters

    result =
      Audit.list_events(
        decision: f["decision"],
        agent_id: f["agent_id"],
        host: f["host"],
        path: f["path"],
        since: f["since"],
        page: socket.assigns.page,
        per_page: @per_page
      )

    socket
    |> assign(
      total: result.total,
      total_pages: max(ceil(result.total / @per_page), 1),
      stats: Audit.stats(since: f["since"]),
      form: to_form(f),
      pending: 0
    )
    |> stream(:events, result.events, reset: true)
  end

  defp normalize_filters(params) do
    filters =
      Map.new(@defaults, fn {key, default} ->
        value = params[key]
        {key, if(is_binary(value), do: String.trim(value), else: default)}
      end)

    filters
    |> Map.update!("since", fn value ->
      if value in (Audit.windows() ++ ["all"]), do: value, else: "1h"
    end)
    |> Map.update!("decision", fn value -> if value in Audit.decisions(), do: value, else: "" end)
  end

  defp page(value) when is_binary(value) do
    case Integer.parse(value) do
      {number, ""} when number > 0 -> min(number, 1_000_000)
      _ -> 1
    end
  end

  defp page(_), do: 1

  def render(assigns) do
    assigns = assign(assigns, :input_class, @input_class)

    ~H"""
    <Layouts.app flash={@flash}>
      <div class="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 class="text-2xl font-semibold tracking-tight text-slate-950">Audit events</h1>
          <p class="mt-2 text-sm text-slate-500">
            Browse the rebuildable index of the proxy's durable JSONL audit trail.
          </p>
        </div>
        <button
          id="toggle-live"
          phx-click="toggle-live"
          class="flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-4 py-2 text-sm font-medium transition hover:bg-slate-50"
        >
          <span class={[
            "size-2 rounded-full",
            if(@live_updates, do: "bg-teal-500", else: "bg-amber-500")
          ]}></span>
          {if @live_updates, do: "Live", else: "Paused"}
          <span :if={@pending > 0}>+{@pending} new</span>
        </button>
      </div>
      <.form
        for={@form}
        id="event-filters"
        phx-change="filter"
        phx-submit="filter"
        class="mt-6 grid items-end gap-3 rounded-xl border border-slate-200 bg-slate-50 p-4 sm:grid-cols-2 lg:grid-cols-6"
      >
        <.input
          field={@form[:decision]}
          type="select"
          label="Decision"
          options={[{"Any", ""}, {"Allow", "allow"}, {"Deny", "deny"}]}
          class={@input_class}
        />
        <.input
          field={@form[:agent_id]}
          label="Agent"
          placeholder="Agent ID"
          phx-debounce="300"
          class={@input_class}
        />
        <.input
          field={@form[:host]}
          label="Host"
          placeholder="github.com"
          phx-debounce="300"
          class={@input_class}
        />
        <.input
          field={@form[:path]}
          label="Path"
          placeholder="/api"
          phx-debounce="300"
          class={@input_class}
        />
        <.input
          field={@form[:since]}
          type="select"
          label="Window"
          options={Enum.map(Audit.windows(), &{"Last #{&1}", &1}) ++ [{"All time", "all"}]}
          class={@input_class}
        />
        <button
          id="reset-filters"
          type="button"
          phx-click="reset"
          class="mb-2 rounded-lg px-4 py-2 text-sm text-slate-600 transition hover:bg-slate-200"
        >Reset filters</button>
      </.form>
      <div class="my-5 flex flex-wrap gap-5 text-sm text-slate-500" aria-live="polite">
        <span id="matching-count">{@total} matching events</span>
        <span class="text-teal-700">{@stats.allow} allowed in window</span>
        <span class="text-rose-700">{@stats.deny} denied in window</span>
      </div>
      <div class="overflow-x-auto rounded-xl border border-slate-200">
        <table class="w-full text-left text-sm">
          <thead class="border-b border-slate-200 bg-slate-50 text-xs uppercase tracking-wide text-slate-500">
            <tr>
              <th class="p-3">Timestamp (UTC)</th><th class="p-3">Decision</th><th class="p-3">
                Agent
              </th><th class="p-3">Request</th><th class="p-3">Status</th><th class="p-3">Latency</th>
            </tr>
          </thead>
          <tbody id="events" phx-update="stream" class="divide-y divide-slate-100">
            <tr id="events-empty" class="hidden only:table-row">
              <td colspan="6" class="p-10 text-center text-slate-500">
                No matching events. Check your filters or wait for the proxy to write an audit record.
              </td>
            </tr>
            <tr :for={{id, event} <- @streams.events} id={id} class="transition hover:bg-slate-50">
              <td class="whitespace-nowrap p-3 font-mono text-xs">
                <.link
                  navigate={~p"/events/#{event.request_id}"}
                  class="text-teal-700 hover:underline"
                >{Calendar.strftime(event.ts, "%Y-%m-%d %H:%M:%S")}</.link>
              </td>
              <td class="p-3">
                <span class={[
                  "rounded-full px-2 py-1 text-xs font-medium",
                  if(event.decision == "deny",
                    do: "bg-rose-50 text-rose-700",
                    else: "bg-teal-50 text-teal-700"
                  )
                ]}>{event.decision}</span>
              </td>
              <td class="p-3 font-mono text-xs">{event.agent_id}</td>
              <td class="max-w-md p-3">
                <.link
                  navigate={~p"/events/#{event.request_id}"}
                  class="block truncate hover:text-teal-700"
                  title={event.path}
                ><span class="font-medium">{event.method}</span> {event.host}{event.path}</.link>
              </td>
              <td class="p-3">{event.status}</td><td class="whitespace-nowrap p-3">
                {Float.round(event.latency_ms, 1)} ms
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <nav aria-label="Pagination" class="mt-5 flex items-center justify-between text-sm">
        <button
          id="newer-page"
          phx-click="page"
          phx-value-page={@page - 1}
          disabled={@page <= 1}
          class="rounded-lg px-4 py-2 transition hover:bg-slate-100 disabled:opacity-40"
        >← Newer</button>
        <span id="page-number" class="text-slate-500">Page {@page} of {@total_pages}</span>
        <button
          id="older-page"
          phx-click="page"
          phx-value-page={@page + 1}
          disabled={@page >= @total_pages}
          class="rounded-lg px-4 py-2 transition hover:bg-slate-100 disabled:opacity-40"
        >Older →</button>
      </nav>
    </Layouts.app>
    """
  end
end
