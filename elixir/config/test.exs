import Config

# Configure your database
#
# The MIX_TEST_PARTITION environment variable can be used
# to provide built-in test partitioning in CI environment.
# Run `mix help test` for more information.
config :leash_collector, LeashCollector.Repo,
  database: Path.expand("../leash_collector_test.db", __DIR__),
  pool_size: 5,
  pool: Ecto.Adapters.SQL.Sandbox

# The ingestion pipeline is exercised through unit tests against the store
# and producer instead of being booted with the test supervision tree.
config :leash_collector, start_audit_pipeline: false

# We don't run a server during test. If one is required,
# you can enable the server option below.
config :leash_collector, LeashCollectorWeb.Endpoint,
  http: [ip: {127, 0, 0, 1}, port: 4002],
  secret_key_base: "0+xXDwYDDpYzFGi91VcYlsQKFdwBgfWEvSJP9TocV7u4cDyeK2Yr68WEXAkPFN9w",
  server: false

# Print only warnings and errors during test
config :logger, level: :warning

# Initialize plugs at runtime for faster test compilation
config :phoenix, :plug_init_mode, :runtime

# Enable helpful, but potentially expensive runtime checks
config :phoenix_live_view,
  enable_expensive_runtime_checks: true

# Sort query params output of verified routes for robust url comparisons
config :phoenix,
  sort_verified_routes_query_params: true
