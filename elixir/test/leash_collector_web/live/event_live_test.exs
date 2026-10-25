defmodule LeashCollectorWeb.EventLiveTest do
  @moduledoc false
  use LeashCollectorWeb.ConnCase, async: false

  import Phoenix.LiveViewTest

  alias LeashCollector.Audit

  defp record(overrides \\ %{}) do
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
        headers: ~s({"user-agent": ["curl/8.0"]}),
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
      Map.new(overrides)
    )
  end

  defp insert!(overrides), do: Audit.insert_batch([record(overrides)])

  describe "home" do
    test "renders the overview with live counts", %{conn: conn} do
      insert!(%{decision: "deny", host: "nope.example"})
      insert!(%{decision: "allow"})

      {:ok, view, html} = live(conn, ~p"/")

      assert html =~ "Proxy audit overview"
      assert view |> has_element?("div", "2")
      assert view |> has_element?("span", "1")
      assert html =~ "nope.example"
    end
  end

  describe "index" do
    test "lists indexed events with newest first", %{conn: conn} do
      insert!(%{
        request_id: "req-old",
        ts: DateTime.add(DateTime.utc_now(), -60, :second) |> DateTime.truncate(:second)
      })

      insert!(%{request_id: "req-new", status: 403})

      {:ok, _view, html} = live(conn, ~p"/events?since=all")

      assert html =~ "req-new"
      assert html =~ "api.github.com"
    end

    test "filters events by decision and host", %{conn: conn} do
      insert!(%{request_id: "req-deny", decision: "deny", host: "evil.example"})
      insert!(%{request_id: "req-ok", host: "api.github.com"})

      {:ok, view, _html} = live(conn, ~p"/events?since=all")

      html =
        view
        |> render_change("filter", %{
          "decision" => "deny",
          "agent_id" => "",
          "host" => "",
          "path" => "",
          "since" => "all"
        })

      assert html =~ "req-deny"
      refute html =~ "req-ok"

      html =
        view
        |> render_change("filter", %{
          "decision" => "",
          "agent_id" => "",
          "host" => "github.com",
          "path" => "",
          "since" => "all"
        })

      assert html =~ "req-ok"
      refute html =~ "req-deny"
    end

    test "paginates", %{conn: conn} do
      for i <- 1..55 do
        insert!(%{request_id: "req-#{i}"})
      end

      {:ok, view, html} = live(conn, ~p"/events?since=all")

      assert html =~ "55 matching events"

      html = render_click(view, :page, %{"page" => "2"})
      assert html =~ "Page 2 of 2"
    end

    test "refreshes the list when a batch is ingested", %{conn: conn} do
      {:ok, view, html} = live(conn, ~p"/events?since=all")
      refute html =~ "req-live"

      insert!(%{request_id: "req-live"})

      assert eventually(fn -> render(view) =~ "req-live" end, 1000)
    end
  end

  describe "show" do
    test "shows a single event with details", %{conn: conn} do
      insert!(%{
        request_id: "req-detail",
        decision: "deny",
        deny_reason: "host not in allowlist",
        path: "/secrets"
      })

      {:ok, _view, html} = live(conn, ~p"/events/req-detail")

      assert html =~ "req-detail"
      assert html =~ "host not in allowlist"
      assert html =~ "/secrets"
    end

    test "redirects for an unknown request id", %{conn: conn} do
      {:error, {:live_redirect, %{to: to}}} = live(conn, ~p"/events/req-missing")

      assert to == ~p"/events"
    end
  end

  defp eventually(fun, timeout) do
    deadline = System.monotonic_time(:millisecond) + timeout

    do_eventually(fun, deadline)
  end

  defp do_eventually(fun, deadline) do
    if fun.() do
      true
    else
      if System.monotonic_time(:millisecond) >= deadline do
        false
      else
        Process.sleep(50)
        do_eventually(fun, deadline)
      end
    end
  end
end
