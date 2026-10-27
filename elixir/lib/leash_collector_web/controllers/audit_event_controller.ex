defmodule LeashCollectorWeb.AuditEventController do
  @moduledoc """
  Authenticated, idempotent ingestion for the Rust proxy's optional HTTP exporter.
  A successful response means the whole batch has been committed to SQLite.
  """
  use LeashCollectorWeb, :controller

  alias LeashCollector.Audit
  alias LeashCollector.Audit.Record

  plug :authenticate

  def create(conn, %{"events" => events}) when is_list(events) and length(events) in 1..200 do
    case normalize(events) do
      {:ok, records} ->
        json(conn, %{inserted: Audit.insert_batch(records)})

      :error ->
        conn |> put_status(:unprocessable_entity) |> json(%{error: "invalid audit batch"})
    end
  end

  def create(conn, _params) do
    conn |> put_status(:unprocessable_entity) |> json(%{error: "expected 1 to 200 audit events"})
  end

  defp authenticate(conn, _opts) do
    case Application.get_env(:leash_collector, :audit_ingest_token) do
      token when is_binary(token) and byte_size(token) > 0 ->
        case get_req_header(conn, "authorization") do
          ["Bearer " <> supplied] ->
            if Plug.Crypto.secure_compare(supplied, token), do: conn, else: unauthorized(conn)

          _ ->
            unauthorized(conn)
        end

      _ ->
        conn
        |> put_status(:service_unavailable)
        |> json(%{error: "HTTP audit ingestion is disabled"})
        |> halt()
    end
  end

  defp unauthorized(conn) do
    conn |> put_status(:unauthorized) |> json(%{error: "unauthorized"}) |> halt()
  end

  defp normalize(events) do
    Enum.reduce_while(events, {:ok, []}, fn event, {:ok, records} ->
      with true <- Record.audit?(event),
           {:ok, record} <- Record.from_json(event) do
        {:cont, {:ok, [record | records]}}
      else
        _ -> {:halt, :error}
      end
    end)
  end
end
