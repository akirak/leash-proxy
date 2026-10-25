defmodule LeashCollectorWeb.HomeLive do
  @moduledoc """
  Landing page: overview of indexed proxy activity with links into the
  event browser. Updates live when the collector ingests a new batch.
  """

  use LeashCollectorWeb, :live_view

  alias LeashCollector.Audit

  def mount(_params, _session, socket) do
    if connected?(socket), do: Audit.subscribe()
    {:ok, assign_stats(socket)}
  end

  def handle_info({:audit_events_inserted, _count}, socket) do
    {:noreply, assign_stats(socket)}
  end

  def handle_info(_other, socket), do: {:noreply, socket}

  defp assign_stats(socket) do
    stats = Audit.stats(since: "24h")

    hosts =
      Enum.map(
        stats.top_denied_hosts,
        &Map.put(&1, :id, Base.url_encode64(&1.host, padding: false))
      )

    socket
    |> assign(:stats, Map.delete(stats, :top_denied_hosts))
    |> assign(:hosts_empty?, hosts == [])
    |> stream(:denied_hosts, hosts, reset: true)
  end

  def render(assigns) do
    ~H"""
    <Layouts.app flash={@flash}>
      <.header>
        Proxy audit overview
        <:subtitle>
          leash-proxy requests indexed from the append-only JSONL audit log into SQLite. Live updates whenever the collector ingests a batch.
        </:subtitle>
        <:actions>
          <.button navigate={~p"/events"}>
            Browse events <.icon name="hero-arrow-right" class="ml-1.5 size-4" />
          </.button>
        </:actions>
      </.header>

      <div class="mt-8 grid gap-6 sm:grid-cols-3">
        <.stat_card id="overview-total" title="Requests (24h)" value={@stats.total} />
        <.stat_card id="overview-allow" title="Allowed (24h)" value={@stats.allow} tone="ok" />
        <.stat_card id="overview-deny" title="Denied (24h)" value={@stats.deny} tone="bad" />
      </div>

      <.card class="mt-10">
        <.card_header>
          <.card_title class="text-base">Top denied hosts (24h)</.card_title>
        </.card_header>
        <.card_content>
          <%= if @hosts_empty? do %>
            <p class="text-sm text-muted-foreground">
              No denied requests indexed yet. Point the collector at the proxy
              audit log and denied requests will show up here.
            </p>
          <% else %>
            <.table>
              <.table_header>
                <.table_row>
                  <.table_head>Host</.table_head>
                  <.table_head>Denied requests</.table_head>
                </.table_row>
              </.table_header>
              <.table_body id="top-denied-hosts" phx-update="stream">
                <.table_row :for={{id, row} <- @streams.denied_hosts} id={id}>
                  <.table_cell>
                    <.link
                      navigate={~p"/events?#{%{decision: "deny", host: row.host, since: "24h"}}"}
                      class="text-teal-700 hover:underline"
                    >{row.host}</.link>
                  </.table_cell>
                  <.table_cell class="font-mono">{row.count}</.table_cell>
                </.table_row>
              </.table_body>
            </.table>
          <% end %>
        </.card_content>
      </.card>
    </Layouts.app>
    """
  end

  attr :id, :string, required: true
  attr :title, :string, required: true
  attr :value, :any, required: true
  attr :tone, :string, default: nil

  defp stat_card(assigns) do
    assigns =
      assign(
        assigns,
        :tone_class,
        %{nil => "text-foreground", "ok" => "text-teal-700", "bad" => "text-rose-700"}[
          assigns.tone
        ]
      )

    ~H"""
    <.card id={@id}>
      <.card_header class="pb-2">
        <.card_description class="text-xs uppercase tracking-wider">{@title}</.card_description>
      </.card_header>
      <.card_content>
        <div class={"text-3xl font-bold " <> @tone_class}>{@value}</div>
      </.card_content>
    </.card>
    """
  end
end
