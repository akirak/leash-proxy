defmodule LeashCollector.Repo.Migrations.CreateAuditEvents do
  use Ecto.Migration

  def change do
    create table(:audit_events, primary_key: false) do
      # SQLite INTEGER PRIMARY KEY; insertion order stays visible for
      # stable pagination on identical timestamps.
      add :id, :bigint, primary_key: true
      add :ts, :utc_datetime, null: false

      add :request_id, :text, null: false
      add :agent_id, :text, null: false
      add :source_ip, :text, default: ""

      add :method, :text, default: ""
      add :scheme, :text, default: ""
      add :host, :text, default: ""
      add :port, :integer, default: 0
      add :path, :text, default: ""
      add :headers, :text, default: ""

      add :decision, :text, null: false, default: ""
      add :engine, :text, default: ""
      add :policy_ids, :text, default: ""
      add :deny_reason, :text, default: ""

      add :status, :integer, default: 0
      add :bytes_in, :bigint, default: 0
      add :bytes_out, :bigint, default: 0
      add :latency_ms, :real, default: 0.0
      add :tls, :text, default: ""
      add :error, :text, default: ""

      add :inserted_at, :utc_datetime, null: false
    end

    # Idempotent batch ingestion (ON CONFLICT DO NOTHING).
    create unique_index(:audit_events, [:request_id])

    # Query paths of the monitoring UI (ADR 0002).
    create index(:audit_events, [:ts])
    create index(:audit_events, [:host])
    create index(:audit_events, [:decision])
    create index(:audit_events, [:agent_id])
  end
end
