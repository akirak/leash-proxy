defmodule LeashCollectorWeb.EventLive.Index do
  @moduledoc "Live audit browser with URL-backed filters, pagination and bounded streams."
  use LeashCollectorWeb, :live_view
  alias LeashCollector.Audit

  @per_page 50
  @defaults %{"decision" => "", "agent_id" => "", "host" => "", "path" => "", "since" => "1h"}

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
    ~H"""
    <Layouts.app flash={@flash}>
      <.header>
        Audit events
        <:subtitle>
          Browse the rebuildable index of the proxy's durable JSONL audit trail.
        </:subtitle>
        <:actions>
          <.button
            id="toggle-live"
            type="button"
            variant="outline"
            phx-click="toggle-live"
            class="gap-2"
          >
            <span class={[
              "size-2 rounded-full",
              if(@live_updates, do: "bg-teal-500", else: "bg-amber-500")
            ]}></span>
            {if @live_updates, do: "Live", else: "Paused"}
            <span :if={@pending > 0}>+{@pending} new</span>
          </.button>
        </:actions>
      </.header>

      <.card class="mt-6">
        <.card_content class="pt-6">
          <.form
            for={@form}
            id="event-filters"
            phx-change="filter"
            phx-submit="filter"
            class="grid items-end gap-3 sm:grid-cols-2 lg:grid-cols-6"
          >
            <.input
              field={@form[:decision]}
              type="select"
              label="Decision"
              options={[{"Any", ""}, {"Allow", "allow"}, {"Deny", "deny"}]}
            />
            <.input
              field={@form[:agent_id]}
              label="Agent"
              placeholder="Agent ID"
              phx-debounce="300"
            />
            <.input
              field={@form[:host]}
              label="Host"
              placeholder="github.com"
              phx-debounce="300"
            />
            <.input
              field={@form[:path]}
              label="Path"
              placeholder="/api"
              phx-debounce="300"
            />
            <.input
              field={@form[:since]}
              type="select"
              label="Window"
              options={Enum.map(Audit.windows(), &{"Last #{&1}", &1}) ++ [{"All time", "all"}]}
            />
            <.button
              id="reset-filters"
              type="button"
              variant="ghost"
              phx-click="reset"
              class="h-10"
            >
              Reset filters
            </.button>
          </.form>
        </.card_content>
      </.card>

      <div
        class="my-5 flex flex-wrap items-center gap-3 text-sm text-muted-foreground"
        aria-live="polite"
      >
        <span id="matching-count">{@total} matching events</span>
        <.badge variant="outline" class="border-teal-200 bg-teal-50 text-teal-700">
          {@stats.allow} allowed in window
        </.badge>
        <.badge variant="outline" class="border-rose-200 bg-rose-50 text-rose-700">
          {@stats.deny} denied in window
        </.badge>
      </div>

      <.card class="overflow-x-auto">
        <.table>
          <.table_header class="bg-muted/50">
            <.table_row>
              <.table_head>Timestamp (UTC)</.table_head>
              <.table_head>Decision</.table_head>
              <.table_head>Agent</.table_head>
              <.table_head>Request</.table_head>
              <.table_head>Status</.table_head>
              <.table_head>Latency</.table_head>
            </.table_row>
          </.table_header>
          <.table_body id="events" phx-update="stream">
            <.table_row id="events-empty" class="hidden only:table-row">
              <td colspan="6" class="p-10 text-center text-muted-foreground">
                No matching events. Check your filters or wait for the proxy to write an audit record.
              </td>
            </.table_row>
            <.table_row :for={{id, event} <- @streams.events} id={id}>
              <.table_cell class="whitespace-nowrap font-mono text-xs">
                <.link
                  navigate={~p"/events/#{event.request_id}"}
                  class="text-teal-700 hover:underline"
                >{Calendar.strftime(event.ts, "%Y-%m-%d %H:%M:%S")}</.link>
              </.table_cell>
              <.table_cell>
                <.badge
                  variant={if(event.decision == "deny", do: "destructive", else: "secondary")}
                  class={
                    if(event.decision == "deny",
                      do: "border-rose-200 bg-rose-50 text-rose-700 hover:bg-rose-50",
                      else: "border-teal-200 bg-teal-50 text-teal-700 hover:bg-teal-50"
                    )
                  }
                >
                  {event.decision}
                </.badge>
              </.table_cell>
              <.table_cell class="font-mono text-xs">{event.agent_id}</.table_cell>
              <.table_cell class="max-w-md">
                <.link
                  navigate={~p"/events/#{event.request_id}"}
                  class="block truncate hover:text-teal-700"
                  title={event.path}
                ><span class="font-medium">{event.method}</span> {event.host}{event.path}</.link>
              </.table_cell>
              <.table_cell>{event.status}</.table_cell>
              <.table_cell class="whitespace-nowrap">
                {Float.round(event.latency_ms, 1)} ms
              </.table_cell>
            </.table_row>
          </.table_body>
        </.table>
      </.card>

      <nav aria-label="Pagination" class="mt-5 flex items-center justify-between text-sm">
        <.button
          id="newer-page"
          type="button"
          variant="outline"
          size="sm"
          phx-click="page"
          phx-value-page={@page - 1}
          disabled={@page <= 1}
        >
          ← Newer
        </.button>
        <span id="page-number" class="text-muted-foreground">Page {@page} of {@total_pages}</span>
        <.button
          id="older-page"
          type="button"
          variant="outline"
          size="sm"
          phx-click="page"
          phx-value-page={@page + 1}
          disabled={@page >= @total_pages}
        >
          Older →
        </.button>
      </nav>
    </Layouts.app>
    """
  end
end
