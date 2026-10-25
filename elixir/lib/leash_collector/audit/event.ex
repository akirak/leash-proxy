defmodule LeashCollector.Audit.Event do
  @moduledoc """
  The schema for indexed leash-proxy audit events (ADR 0002).

  One row per proxied request. The append-only JSONL log written by the Rust
  proxy remains the durable source of truth; this table is a rebuildable
  index for the monitoring UI. `request_id` is unique so batch ingestion is
  idempotent (`ON CONFLICT DO NOTHING`).
  """

  use Ecto.Schema

  schema "audit_events" do
    field :ts, :utc_datetime

    field :request_id, :string
    field :agent_id, :string
    field :source_ip, :string

    field :method, :string
    field :scheme, :string
    field :host, :string
    field :port, :integer
    # Redacted by the proxy: no query strings, allowlisted headers only.
    field :path, :string
    field :headers, :string

    field :decision, :string
    field :engine, :string
    field :policy_ids, :string
    field :deny_reason, :string

    field :status, :integer
    field :bytes_in, :integer
    field :bytes_out, :integer
    field :latency_ms, :float
    field :tls, :string
    field :error, :string

    timestamps(type: :utc_datetime, inserted_at: :inserted_at, updated_at: false)
  end
end
