defmodule LeashCollectorWeb.EventLiveTest do
  use LeashCollectorWeb.ConnCase, async: false
  import Phoenix.LiveViewTest
  alias LeashCollector.Audit

  defp insert!(overrides) do
    Audit.insert_batch([
      Map.merge(
        %{
          ts: DateTime.utc_now() |> DateTime.truncate(:second),
          request_id: "req-#{System.unique_integer([:positive])}",
          agent_id: "agent-1",
          source_ip: "10.0.0.5",
          method: "GET",
          scheme: "https",
          host: "api.github.com",
          port: 443,
          path: "/api/v3/user",
          headers: "{}",
          decision: "allow",
          engine: "cedar",
          policy_ids: "p1",
          deny_reason: "",
          status: 200,
          bytes_in: 100,
          bytes_out: 200,
          latency_ms: 10.0,
          tls: "intercepted",
          error: ""
        },
        overrides
      )
    ])
  end

  test "overview shows counts and denied hosts", %{conn: conn} do
    insert!(%{decision: "deny", host: "nope.example"})
    insert!(%{decision: "allow"})
    {:ok, view, _} = live(conn, ~p"/")
    assert has_element?(view, "#overview-total", "2")
    assert has_element?(view, "#overview-allow", "1")
    assert has_element?(view, "#overview-deny", "1")
    assert has_element?(view, "#top-denied-hosts", "nope.example")
  end

  test "URL filters are applied, including historical events", %{conn: conn} do
    insert!(%{request_id: "old", ts: ~U[2020-01-01 00:00:00Z], host: "old.example"})
    insert!(%{request_id: "new"})
    {:ok, view, _} = live(conn, ~p"/events?since=all&host=old.example")
    assert has_element?(view, "#matching-count", "1 matching events")
    assert has_element?(view, "#events", "old.example")
    refute has_element?(view, "#events", "api.github.com")
  end

  test "form filters by decision and reset restores defaults", %{conn: conn} do
    insert!(%{decision: "deny", host: "evil.example"})
    insert!(%{})
    {:ok, view, _} = live(conn, ~p"/events?since=all")
    view |> form("#event-filters", %{"decision" => "deny"}) |> render_change()
    assert has_element?(view, "#events", "evil.example")
    refute has_element?(view, "#events", "api.github.com")
    view |> element("#reset-filters") |> render_click()
    assert has_element?(view, "#matching-count", "2 matching events")
  end

  test "paginates and tolerates invalid URL pages", %{conn: conn} do
    for i <- 1..55, do: insert!(%{request_id: "req-#{i}"})
    {:ok, view, _} = live(conn, ~p"/events?since=all&page=invalid")
    assert has_element?(view, "#matching-count", "55 matching events")
    assert row_count(view) == 50
    view |> element("#older-page") |> render_click()
    assert has_element?(view, "#page-number", "Page 2 of 2")
    assert row_count(view) == 5
  end

  test "live updates pause and catch up on resume", %{conn: conn} do
    {:ok, view, _} = live(conn, ~p"/events?since=all")
    insert!(%{host: "live.example"})
    send(view.pid, :refresh)
    assert has_element?(view, "#events", "live.example")
    view |> element("#toggle-live") |> render_click()
    insert!(%{host: "paused.example"})
    send(view.pid, :refresh)
    refute has_element?(view, "#events", "paused.example")
    assert has_element?(view, "#toggle-live", "Paused")
    view |> element("#toggle-live") |> render_click()
    assert has_element?(view, "#events", "paused.example")
  end

  test "shows full event details", %{conn: conn} do
    insert!(%{
      request_id: "detail",
      decision: "deny",
      deny_reason: "host not in allowlist",
      path: "/secrets"
    })

    {:ok, view, _} = live(conn, ~p"/events/detail")
    assert has_element?(view, "#event-detail", "host not in allowlist")
    assert has_element?(view, "#event-detail", "/secrets")
  end

  defp row_count(view) do
    view
    |> render()
    |> LazyHTML.from_document()
    |> LazyHTML.query("#events tr:not(#events-empty)")
    |> Enum.count()
  end

  test "redirects for an unknown request", %{conn: conn} do
    assert {:error, {:live_redirect, %{to: "/events"}}} = live(conn, ~p"/events/missing")
  end
end
