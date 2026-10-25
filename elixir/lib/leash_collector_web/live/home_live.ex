defmodule LeashCollectorWeb.HomeLive do
  @moduledoc """
  Landing page: overview of indexed proxy activity with links into the
  event browser. Updates live when the collector ingests a new batch.
  """

  use LeashCollectorWeb, :live_view

  alias LeashCollector.Audit

  def mount(_params, _session, socket) do
    Audit.subscribe()
    {:ok, assign_stats(socket)}
  end

  def handle_info({:audit_events_inserted, _count}, socket) do
    {:noreply, assign_stats(socket)}
  end

  def handle_info(_other, socket), do: {:noreply, socket}

  defp assign_stats(socket) do
    assign(socket, :stats, Audit.stats(since: "24h"))
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
            Browse events <.icon name="hero-arrow-right" />
          </.button>
        </:actions>
      </.header>

      <div class="mt-8 grid gap-6 sm:grid-cols-3">
        <.stat_card title="Requests (24h)" value={@stats.total} />
        <.stat_card title="Allowed (24h)" value={@stats.allow} tone="ok" />
        <.stat_card title="Denied (24h)" value={@stats.deny} tone="bad" />
      </div>

      <div class="mt-10">
        <h2 class="pb-2 font-semibold">Top denied hosts (24h)</h2>

        <%= if @stats.top_denied_hosts == [] do %>
          <p class="text-sm text-base-content/70">
            No denied requests indexed yet. Point the collector at the proxy
            audit log and denied requests will show up here.
          </p>
        <% else %>
          <.table id="top-denied-hosts" rows={@stats.top_denied_hosts}>
            <:col :let={row} label="Host">{row.host}</:col>
            <:col :let={row} label="Denied requests">{row.count}</:col>
          </.table>
        <% end %>
      </div>
    </Layouts.app>
    """
  end

  attr :title, :string, required: true
  attr :value, :any, required: true
  attr :tone, :string, default: nil

  defp stat_card(assigns) do
    assigns =
      assign(
        assigns,
        :tone_class,
        %{nil => "", "ok" => "text-success", "bad" => "text-error"}[assigns.tone]
      )

    ~H"""
    <div class="card bg-base-200 shadow-sm">
      <div class="card-body">
        <div class="text-sm uppercase tracking-wide text-base-content/60">{@title}</div>
        <div class={"text-3xl font-bold " <> @tone_class}>{@value}</div>
      </div>
    </div>
    """
  end
end
