defmodule LeashCollector.Application do
  # See https://elixir.hexdocs.pm/Application.html
  # for more information on OTP Applications
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    children =
      [
        LeashCollectorWeb.Telemetry,
        LeashCollector.Repo,
        {Ecto.Migrator,
         repos: Application.fetch_env!(:leash_collector, :ecto_repos), skip: skip_migrations?()},
        {DNSCluster, query: Application.get_env(:leash_collector, :dns_cluster_query) || :ignore},
        {Phoenix.PubSub, name: LeashCollector.PubSub}
      ] ++
        audit_pipeline() ++
        [
          # Start to serve requests, typically the last entry
          LeashCollectorWeb.Endpoint
        ]

    # See https://elixir.hexdocs.pm/Supervisor.html
    # for other strategies and supported options
    opts = [strategy: :one_for_one, name: LeashCollector.Supervisor]
    Supervisor.start_link(children, opts)
  end

  defp audit_pipeline do
    if Application.get_env(:leash_collector, :start_audit_pipeline, true) do
      [{LeashCollector.AuditPipeline, []}]
    else
      []
    end
  end

  # Tell Phoenix to update the endpoint configuration
  # whenever the application is updated.
  @impl true
  def config_change(changed, _new, removed) do
    LeashCollectorWeb.Endpoint.config_change(changed, removed)
    :ok
  end

  defp skip_migrations?() do
    # By default, sqlite migrations are run when using a release
    System.get_env("RELEASE_NAME") == nil
  end
end
