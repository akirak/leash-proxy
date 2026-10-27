defmodule LeashCollectorWeb.AuditEventControllerTest do
  use LeashCollectorWeb.ConnCase

  alias LeashCollector.Audit

  setup do
    previous = Application.get_env(:leash_collector, :audit_ingest_token)
    Application.put_env(:leash_collector, :audit_ingest_token, "test-token")

    on_exit(fn ->
      if previous do
        Application.put_env(:leash_collector, :audit_ingest_token, previous)
      else
        Application.delete_env(:leash_collector, :audit_ingest_token)
      end
    end)

    :ok
  end

  defp event(id) do
    %{
      "target" => "leash_proxy::audit",
      "request_id" => id,
      "timestamp" => "2026-10-27T10:00:00Z",
      "agent_id" => "vm-alpha",
      "host" => "api.github.com",
      "method" => "GET",
      "decision" => "deny",
      "path" => "/repos",
      "status" => 403
    }
  end

  defp ingest(events, token \\ "test-token") do
    build_conn()
    |> put_req_header("authorization", "Bearer #{token}")
    |> post(~p"/api/audit/events", %{events: events})
  end

  test "commits a normalized batch, broadcasts, and deduplicates retries" do
    Audit.subscribe()
    assert %{"inserted" => 2} = ingest([event("one"), event("two")]) |> json_response(200)
    assert_receive {:audit_events_inserted, 2}
    assert %{path: "/repos", status: 403, ts: ~U[2026-10-27 10:00:00Z]} = Audit.get_event("one")
    assert %{"inserted" => 0} = ingest([event("one"), event("two")]) |> json_response(200)
    refute_receive {:audit_events_inserted, _}
  end

  test "requires the shared bearer token", %{conn: conn} do
    assert ingest([event("nope")], "wrong") |> json_response(401)
    assert conn |> post(~p"/api/audit/events", %{events: [event("nope")]}) |> json_response(401)
    assert is_nil(Audit.get_event("nope"))
  end

  test "is disabled without a nonempty configured token" do
    for token <- [nil, ""] do
      Application.put_env(:leash_collector, :audit_ingest_token, token)
      assert ingest([event("disabled")]) |> json_response(503)
    end

    assert is_nil(Audit.get_event("disabled"))
  end

  test "rejects a whole batch before inserting any malformed or operational events" do
    for bad <- [
          nil,
          "invalid",
          %{},
          Map.put(event("bad"), "status", -1),
          Map.put(event("bad"), "target", "leash_proxy")
        ] do
      assert ingest([event("valid"), bad]) |> json_response(422)
      assert is_nil(Audit.get_event("valid"))
    end
  end

  test "bounds batch size and rejects invalid envelopes", %{conn: conn} do
    for events <- [[], List.duplicate(event("too-many"), 201), "not-a-list"] do
      assert ingest(events) |> json_response(422)
    end

    assert conn
           |> put_req_header("authorization", "Bearer test-token")
           |> post(~p"/api/audit/events", %{})
           |> json_response(422)
  end
end
