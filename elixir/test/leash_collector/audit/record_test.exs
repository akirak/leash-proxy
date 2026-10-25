defmodule LeashCollector.Audit.RecordTest do
  @moduledoc false
  use ExUnit.Case, async: true

  alias LeashCollector.Audit.Record

  @audit_event %{
    "timestamp" => "2026-10-25T12:00:00.123456Z",
    "target" => "leash_proxy::audit",
    "level" => "INFO",
    "request_id" => "req-1",
    "agent_id" => "agent-vm-1",
    "source_ip" => "10.0.0.5",
    "method" => "GET",
    "scheme" => "https",
    "host" => "api.github.com",
    "port" => 443,
    "path" => "/api/v3/user",
    "headers" => ~s({"user-agent": ["curl/8.0"]}),
    "decision" => "allow",
    "engine" => "cedar",
    "policy_ids" => "p1,p2",
    "deny_reason" => "",
    "status" => 200,
    "bytes_in" => 512,
    "bytes_out" => 1024,
    "latency_ms" => 12.5,
    "tls" => "intercepted",
    "error" => ""
  }

  test "audit?/1 matches only leash-proxy audit targets" do
    assert Record.audit?(%{"target" => "leash_proxy::audit"})
    refute Record.audit?(%{"target" => "leash_proxy::proxy"})
    refute Record.audit?(%{"target" => nil})
  end

  test "from_json/1 normalizes a proxy audit record" do
    assert {:ok, params} = Record.from_json(@audit_event)

    assert params.ts == ~U[2026-10-25 12:00:00Z]
    assert params.request_id == "req-1"
    assert params.agent_id == "agent-vm-1"
    assert params.method == "GET"
    assert params.host == "api.github.com"
    assert params.port == 443
    assert params.decision == "allow"
    assert params.policy_ids == "p1,p2"
    assert params.status == 200
    assert params.bytes_in == 512
    assert params.latency_ms == 12.5
  end

  test "from_json/1 falls back to now for missing/invalid timestamps" do
    json = Map.merge(@audit_event, %{"timestamp" => nil, "ts" => "not-a-date"})

    assert {:ok, params} = Record.from_json(json)

    assert DateTime.diff(DateTime.utc_now(), params.ts, :second) < 5
  end

  test "from_json/1 accepts plain ts keys and joins list policy ids" do
    json =
      Map.merge(@audit_event, %{"timestamp" => nil, "ts" => "2026-10-25T12:00:00Z"})
      |> Map.put("policy_ids", ["p1", "p2"])

    assert {:ok, params} = Record.from_json(json)
    assert params.ts == ~U[2026-10-25 12:00:00Z]
    assert params.policy_ids == "p1,p2"
  end

  test "from_json/1 defaults missing optional fields" do
    json =
      Map.take(@audit_event, [
        "target",
        "request_id",
        "agent_id",
        "method",
        "host",
        "decision",
        "timestamp"
      ])

    assert {:ok, params} = Record.from_json(json)

    assert params.status == 0
    assert params.bytes_out == 0
    assert params.latency_ms == 0.0
    assert params.error == ""
    assert params.deny_reason == ""
  end

  test "malformed field types return errors instead of crashing processors" do
    for {field, value} <- [
          {"status", "not-a-number"},
          {"port", %{}},
          {"latency_ms", []},
          {"bytes_in", -1},
          {"bytes_out", 9_223_372_036_854_775_808},
          {"host", %{}},
          {"headers", %{}},
          {"policy_ids", [%{}]},
          {"decision", "pending"}
        ] do
      assert {:error, _} = Record.from_json(Map.put(@audit_event, field, value))
    end

    assert {:error, :not_an_object} = Record.from_json(nil)
  end

  test "from_json/1 rejects records with missing required fields" do
    assert {:error, {:missing_fields, missing}} =
             Record.from_json(%{"target" => "leash_proxy::audit"})

    assert "request_id" in missing
    assert "agent_id" in missing
    assert "decision" in missing
    assert "host" in missing
    assert "method" in missing
  end
end
