defmodule LeashCollector.AuditTest do
  @moduledoc false
  use LeashCollector.DataCase, async: true

  alias LeashCollector.Audit
  alias LeashCollector.Repo

  defp record(overrides \\ %{}) do
    Map.merge(
      %{
        ts: ~U[2026-10-25 12:00:00Z],
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
        tls: "passthrough",
        error: ""
      },
      Map.new(overrides)
    )
  end

  defp insert!(overrides), do: insert_batch!([record(overrides)])

  defp insert_batch!(records) do
    count = Audit.insert_batch(records)
    assert count == length(records)
    count
  end

  test "insert_batch/1 inserts rows and broadcasts a notification" do
    Audit.subscribe()

    insert_batch!([record(), record(%{decision: "deny", deny_reason: "not allowed"})])

    assert Repo.aggregate(Audit.Event, :count) == 2
    assert_receive {:audit_events_inserted, 2}
  end

  test "insert_batch/1 is idempotent on request_id conflicts" do
    shared = record(%{request_id: "req-dup"})
    insert_batch!([shared])
    # A replay of the same event is skipped, not duplicated: only the
    # genuinely new record is inserted and returned.
    assert Audit.insert_batch([shared, record()]) == 1

    assert Repo.aggregate(Audit.Event, :count) == 2
  end

  test "insert_batch/1 returns 0 for an empty batch" do
    assert Audit.insert_batch([]) == 0
  end

  test "list_events/1 orders newest first and paginates" do
    for i <- 1..5 do
      insert!(%{ts: ~U[2026-10-25 12:00:00Z] |> DateTime.add(i, :minute)})
    end

    page1 = Audit.list_events(page: 1, per_page: 3, since: "all")
    assert page1.total == 5
    assert length(page1.events) == 3
    assert page1.events |> hd() |> Map.get(:ts) == ~U[2026-10-25 12:05:00Z]

    page2 = Audit.list_events(page: 2, per_page: 3, since: "all")
    assert length(page2.events) == 2
  end

  test "list_events/1 filters by decision, agent, host, and path" do
    insert!(%{decision: "deny", deny_reason: "unknown host", host: "evil.example", path: "/x"})
    insert!(%{host: "api.github.com", path: "/api/v3/user", agent_id: "agent-2"})
    insert!(%{decision: "deny", host: "api.github.com", agent_id: "agent-2"})

    assert %{total: 2} = Audit.list_events(decision: "deny", since: "all")
    assert %{total: 2} = Audit.list_events(host: "github.com", since: "all")
    assert %{total: 1} = Audit.list_events(decision: "deny", host: "evil.example", since: "all")
    assert %{total: 2} = Audit.list_events(agent_id: "agent-2", since: "all")
    assert %{total: 2} = Audit.list_events(path: "/api/v3", since: "all")
    assert %{total: 1} = Audit.list_events(path: "/x", since: "all")
    # Substring % in filter input must not act as a wildcard.
    assert %{total: 0} = Audit.list_events(host: "%", since: "all")
  end

  test "list_events/1 filters by time window" do
    insert!(%{ts: DateTime.utc_now() |> DateTime.add(-30, :minute) |> DateTime.truncate(:second)})
    insert!(%{ts: ~U[2020-01-01 00:00:00Z]})

    assert %{total: 1} = Audit.list_events(since: "1h")
    assert %{total: 2} = Audit.list_events(since: "all")
  end

  test "get_event/1 fetches by request id" do
    insert!(%{request_id: "req-known"})

    assert %LeashCollector.Audit.Event{request_id: "req-known"} = Audit.get_event("req-known")
    assert Audit.get_event("req-missing") == nil
  end

  test "stats/1 aggregates by decision and top denied hosts" do
    now = DateTime.utc_now() |> DateTime.truncate(:second)

    insert!(%{decision: "deny", host: "a.example", ts: now})
    insert!(%{decision: "deny", host: "a.example", ts: now})
    insert!(%{decision: "deny", host: "b.example", ts: now})
    insert!(%{decision: "allow", host: "api.github.com", ts: now})
    insert!(%{ts: ~U[2020-01-01 00:00:00Z]})

    stats = Audit.stats(since: "1h")

    assert stats.total == 4
    assert stats.allow == 1
    assert stats.deny == 3

    assert [%{host: "a.example", count: 2}, %{host: "b.example", count: 1}] =
             stats.top_denied_hosts
  end

  test "prune_before/1 deletes older events" do
    insert!(%{ts: ~U[2020-01-01 00:00:00Z]})
    insert!(%{ts: ~U[2026-10-25 12:00:00Z]})

    assert Audit.prune_before(~U[2021-01-01 00:00:00Z]) == 1
    assert Repo.aggregate(Audit.Event, :count) == 1
  end
end
