defmodule LeashCollectorWeb.Router do
  use LeashCollectorWeb, :router

  pipeline :browser do
    plug :accepts, ["html"]
    plug :fetch_session
    plug :fetch_live_flash
    plug :put_root_layout, html: {LeashCollectorWeb.Layouts, :root}
    plug :protect_from_forgery
    plug :put_secure_browser_headers
  end

  pipeline :api do
    plug :accepts, ["json"]
  end

  scope "/", LeashCollectorWeb do
    pipe_through :browser

    live "/", HomeLive, :index
    live "/events", EventLive.Index, :index
    live "/events/:request_id", EventLive.Show, :show
  end

  # Other scopes may use custom stacks.
  # scope "/api", LeashCollectorWeb do
  #   pipe_through :api
  # end

  # Enable LiveDashboard in development
  if Application.compile_env(:leash_collector, :dev_routes) do
    # If you want to use the LiveDashboard in production, you should put
    # it behind authentication and allow only admins to access it.
    # If your application does not have an admins-only section yet, you
    # can use the use Plug.BasicAuth to set up some basic authentication
    # as long as you are also using SSL (which you should anyway).
    import Phoenix.LiveDashboard.Router

    scope "/dev" do
      pipe_through :browser

      live_dashboard "/dashboard", metrics: LeashCollectorWeb.Telemetry
    end
  end
end
