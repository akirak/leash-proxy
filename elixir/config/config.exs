# This file is responsible for configuring your application
# and its dependencies with the aid of the Config module.
#
# This configuration file is loaded before any dependency and
# is restricted to this project.

# General application configuration
import Config

config :leash_collector,
  ecto_repos: [LeashCollector.Repo],
  generators: [timestamp_type: :utc_datetime],
  # Ingestion (ADR 0003): the JSONL audit log written by the Rust proxy
  # (`leash-proxy --log-file PATH`) is tailed from the host filesystem.
  # A message-broker producer can replace the tail producer later without
  # touching the rest of the pipeline.
  audit_log_path: System.get_env("LEASH_AUDIT_LOG", "tmp/audit.jsonl"),
  audit_cursor_path: System.get_env("LEASH_AUDIT_CURSOR") || nil,
  audit_poll_interval: 500,
  start_audit_pipeline: true

# Configure the endpoint
config :leash_collector, LeashCollectorWeb.Endpoint,
  url: [host: "localhost"],
  adapter: Bandit.PhoenixAdapter,
  render_errors: [
    formats: [html: LeashCollectorWeb.ErrorHTML, json: LeashCollectorWeb.ErrorJSON],
    layout: false
  ],
  pubsub_server: LeashCollector.PubSub,
  live_view: [signing_salt: "4BNHNjlh"]

# Configure LiveView
config :phoenix_live_view,
  # the attribute set on all root tags. Used for Phoenix.LiveView.ColocatedCSS.
  root_tag_attribute: "phx-r"

# Configure esbuild (the version is required)
config :esbuild,
  version: "0.25.4",
  leash_collector: [
    args:
      ~w(js/app.js --bundle --target=es2022 --outdir=../priv/static/assets/js --external:/fonts/* --external:/images/* --alias:@=.),
    cd: Path.expand("../assets", __DIR__),
    env: %{"NODE_PATH" => [Path.expand("../deps", __DIR__), Mix.Project.build_path()]}
  ]

# Configure tailwind (the version is required)
config :tailwind,
  version: "4.3.0",
  leash_collector: [
    args: ~w(
      --input=assets/css/app.css
      --output=priv/static/assets/css/app.css
    ),
    cd: Path.expand("..", __DIR__),
    env: %{"NODE_PATH" => [Path.expand("../deps", __DIR__), Mix.Project.build_path()]}
  ]

# Configure Elixir's Logger
config :logger, :default_formatter,
  format: "$time $metadata[$level] $message\n",
  metadata: [:request_id]

# Use Jason for JSON parsing in Phoenix
config :phoenix, :json_library, Jason

# Configure SaladUI error translator
config :salad_ui, :error_translator_function, {LeashCollectorWeb.CoreComponents, :translate_error}

# Import environment specific config. This must remain at the bottom
# of this file so it overrides the configuration defined above.
import_config "#{config_env()}.exs"
