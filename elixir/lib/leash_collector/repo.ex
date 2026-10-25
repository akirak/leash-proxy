defmodule LeashCollector.Repo do
  use Ecto.Repo,
    otp_app: :leash_collector,
    adapter: Ecto.Adapters.SQLite3
end
