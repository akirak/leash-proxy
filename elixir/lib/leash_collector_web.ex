defmodule LeashCollectorWeb do
  @moduledoc """
  The entrypoint for defining your web interface, such
  as controllers, components, channels, and so on.

  This can be used in your application as:

      use LeashCollectorWeb, :controller
      use LeashCollectorWeb, :html

  The definitions below will be executed for every controller,
  component, etc, so keep them short and clean, focused
  on imports, uses and aliases.

  Do NOT define functions inside the quoted expressions
  below. Instead, define additional modules and import
  those modules here.
  """

  def static_paths, do: ~w(assets fonts images favicon.ico robots.txt)

  def router do
    quote do
      use Phoenix.Router, helpers: false

      # Import common connection and controller functions to use in pipelines
      import Plug.Conn
      import Phoenix.Controller
      import Phoenix.LiveView.Router
    end
  end

  def channel do
    quote do
      use Phoenix.Channel
    end
  end

  def controller do
    quote do
      use Phoenix.Controller, formats: [:html, :json]

      use Gettext, backend: LeashCollectorWeb.Gettext

      import Plug.Conn

      unquote(verified_routes())
    end
  end

  def live_view do
    quote do
      use Phoenix.LiveView

      unquote(html_helpers())
    end
  end

  def live_component do
    quote do
      use Phoenix.LiveComponent

      unquote(html_helpers())
    end
  end

  def html do
    quote do
      use Phoenix.Component

      # Import convenience functions from controllers
      import Phoenix.Controller,
        only: [get_csrf_token: 0, view_module: 1, view_template: 1]

      # Include general helpers for rendering HTML
      unquote(html_helpers())
    end
  end

  defp html_helpers do
    quote do
      # Translation
      use Gettext, backend: LeashCollectorWeb.Gettext

      # HTML escaping functionality
      import Phoenix.HTML
      # SaladUI components
      import SaladUI.Accordion
      import SaladUI.Alert
      import SaladUI.AlertDialog
      import SaladUI.Avatar
      import SaladUI.Badge
      import SaladUI.Breadcrumb
      import SaladUI.Card
      import SaladUI.Chart
      import SaladUI.Checkbox
      import SaladUI.Collapsible
      import SaladUI.Dialog
      import SaladUI.DropdownMenu
      import SaladUI.Form
      import SaladUI.Helpers
      import SaladUI.HoverCard
      import SaladUI.Label
      import SaladUI.Menu
      import SaladUI.Pagination
      import SaladUI.Popover
      import SaladUI.Progress
      import SaladUI.RadioGroup
      import SaladUI.ScrollArea
      import SaladUI.Select
      import SaladUI.Separator
      import SaladUI.Sheet
      import SaladUI.Sidebar
      import SaladUI.Skeleton
      import SaladUI.Slider
      import SaladUI.Switch
      import SaladUI.Table
      import SaladUI.Tabs
      import SaladUI.Textarea
      import SaladUI.Toast
      import SaladUI.Toggle
      import SaladUI.ToggleGroup
      import SaladUI.Tooltip
      # Core UI components
      import LeashCollectorWeb.CoreComponents

      # Common modules used in templates
      alias Phoenix.LiveView.JS
      alias LeashCollectorWeb.Layouts

      # Routes generation with the ~p sigil
      unquote(verified_routes())
    end
  end

  def verified_routes do
    quote do
      use Phoenix.VerifiedRoutes,
        endpoint: LeashCollectorWeb.Endpoint,
        router: LeashCollectorWeb.Router,
        statics: LeashCollectorWeb.static_paths()
    end
  end

  @doc """
  When used, dispatch to the appropriate controller/live_view/etc.
  """
  defmacro __using__(which) when is_atom(which) do
    apply(__MODULE__, which, [])
  end
end
