defmodule LeashCollector.Audit.Record do
  @moduledoc """
  Decodes raw leash-proxy JSONL log events into normalized event params.

  The proxy (see `rust/src/audit.rs`) emits one flattened JSON object per
  proxied request with target `leash_proxy::audit`. The collector only
  indexes those audit events; operational log lines from the same file are
  skipped. All decoding/validation happens upstream of the database sink so
  batch insertion stays cheap.
  """

  @audit_target "leash_proxy::audit"

  @doc """
  Returns `true` when the decoded JSON object is a leash-proxy audit event.
  """
  def audit?(%{"target" => @audit_target}), do: true
  def audit?(_), do: false

  @doc """
  Normalizes a decoded JSON object into `Event` params.

  Returns `{:ok, params}` or `{:error, reason}` for malformed fields.
  Absent optional fields receive empty defaults and unknown fields are
  ignored. Integers must fit SQLite's signed 64-bit range.
  """
  def from_json(json) when is_map(json) do
    with :ok <- require!(json),
         :ok <- validate_types(json),
         {:ok, ts} <- parse_ts(json) do
      {:ok,
       %{
         ts: ts,
         request_id: string(json, "request_id"),
         agent_id: string(json, "agent_id"),
         source_ip: string(json, "source_ip"),
         method: string(json, "method"),
         scheme: string(json, "scheme"),
         host: string(json, "host"),
         port: int(json, "port"),
         path: string(json, "path"),
         headers: string(json, "headers"),
         decision: string(json, "decision"),
         engine: string(json, "engine"),
         policy_ids: string(json, "policy_ids"),
         deny_reason: string(json, "deny_reason"),
         status: int(json, "status"),
         bytes_in: int(json, "bytes_in"),
         bytes_out: int(json, "bytes_out"),
         latency_ms: float(json, "latency_ms"),
         tls: string(json, "tls"),
         error: string(json, "error")
       }}
    end
  end

  def from_json(_), do: {:error, :not_an_object}

  defp require!(json) do
    missing =
      Enum.filter(
        ~w(request_id agent_id decision host method),
        &(not is_binary(json[&1]) or json[&1] == "")
      )

    if missing == [] do
      :ok
    else
      {:error, {:missing_fields, missing}}
    end
  end

  defp validate_types(json) do
    strings =
      ~w(request_id agent_id source_ip method scheme host path headers engine deny_reason tls error)

    integers = ~w(port status bytes_in bytes_out)

    invalid =
      Enum.find(strings, fn key -> not (is_nil(json[key]) or is_binary(json[key])) end) ||
        Enum.find(integers, fn key ->
          value = json[key]

          not (is_nil(value) or
                 (is_integer(value) and value >= 0 and value <= 9_223_372_036_854_775_807))
        end) ||
        if(
          not (is_nil(json["latency_ms"]) or
                 (is_number(json["latency_ms"]) and json["latency_ms"] >= 0 and
                    json["latency_ms"] <= 9_223_372_036_854_775_807)),
          do: "latency_ms"
        ) ||
        if(
          not (is_nil(json["policy_ids"]) or is_binary(json["policy_ids"]) or
                 (is_list(json["policy_ids"]) and Enum.all?(json["policy_ids"], &is_binary/1))),
          do: "policy_ids"
        ) ||
        if(json["decision"] not in ~w(allow deny), do: "decision")

    if invalid, do: {:error, {:invalid_field, invalid}}, else: :ok
  end

  defp parse_ts(json) do
    raw = json["ts"] || json["timestamp"]

    case is_binary(raw) && DateTime.from_iso8601(raw) do
      {:ok, ts, _offset} -> {:ok, DateTime.truncate(ts, :second)}
      _ -> {:ok, DateTime.utc_now() |> DateTime.truncate(:second)}
    end
  end

  defp string(json, key) do
    case json[key] do
      nil -> ""
      value when is_binary(value) -> value
      value when is_list(value) -> Enum.join(value, ",")
    end
  end

  defp int(json, key) do
    case json[key] do
      nil -> 0
      value when is_integer(value) -> value
    end
  end

  defp float(json, key) do
    case json[key] do
      nil -> 0.0
      value when is_number(value) -> value * 1.0
    end
  end
end
