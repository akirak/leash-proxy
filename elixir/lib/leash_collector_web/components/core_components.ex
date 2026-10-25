defmodule LeashCollectorWeb.CoreComponents do
  @moduledoc """
  Provides core UI components built on SaladUI and Tailwind CSS.

  Here are useful references:

    * [SaladUI](https://hexdocs.pm/salad_ui) - Phoenix LiveView component
      library inspired by shadcn/ui.

    * [Tailwind CSS](https://tailwindcss.com) - the foundational framework
      we build on. You will use it for layout, sizing, flexbox, grid, and
      spacing.

    * [Heroicons](https://heroicons.com) - see `icon/1` for usage.

    * [Phoenix.Component](https://phoenix-live-view.hexdocs.pm/Phoenix.Component.html) -
      the component system used by Phoenix. Some components, such as `<.link>`
      and `<.form>`, are defined there.

  """
  use Phoenix.Component
  use Gettext, backend: LeashCollectorWeb.Gettext

  alias Phoenix.LiveView.JS

  @doc """
  Renders flash notices using `SaladUI.Alert`.

  ## Examples

      <.flash kind={:info} flash={@flash} />
      <.flash
        id="welcome-back"
        kind={:info}
        phx-mounted={show("#welcome-back") |> JS.remove_attribute("hidden")}
        hidden
      >
        Welcome Back!
      </.flash>
  """
  attr :id, :string, doc: "the optional id of flash container"
  attr :flash, :map, default: %{}, doc: "the map of flash messages to display"
  attr :title, :string, default: nil
  attr :kind, :atom, values: [:info, :error], doc: "used for styling and flash lookup"
  attr :rest, :global, doc: "the arbitrary HTML attributes to add to the flash container"

  slot :inner_block, doc: "the optional inner block that renders the flash message"

  def flash(assigns) do
    assigns = assign_new(assigns, :id, fn -> "flash-#{assigns.kind}" end)

    ~H"""
    <div
      :if={msg = render_slot(@inner_block) || Phoenix.Flash.get(@flash, @kind)}
      id={@id}
      phx-click={JS.push("lv:clear-flash", value: %{key: @kind}) |> hide("##{@id}")}
      class="fixed top-4 right-4 z-50 w-80 max-w-80 sm:w-96 sm:max-w-96"
      {@rest}
    >
      <SaladUI.Alert.alert
        variant={if(@kind == :error, do: "destructive", else: "default")}
        class="flex items-start gap-3 shadow-lg [&>span~*]:pl-0 [&>span]:static"
      >
        <.icon :if={@kind == :info} name="hero-information-circle" class="size-5 shrink-0" />
        <.icon :if={@kind == :error} name="hero-exclamation-circle" class="size-5 shrink-0" />
        <div class="flex-1 text-wrap">
          <SaladUI.Alert.alert_title :if={@title}>{@title}</SaladUI.Alert.alert_title>
          <SaladUI.Alert.alert_description>{msg}</SaladUI.Alert.alert_description>
        </div>
        <button type="button" class="group self-start cursor-pointer" aria-label={gettext("close")}>
          <.icon name="hero-x-mark" class="size-5 opacity-40 group-hover:opacity-70" />
        </button>
      </SaladUI.Alert.alert>
    </div>
    """
  end

  @doc """
  Renders a SaladUI button with optional navigation support (`href`, `navigate`, `patch`).

  ## Examples

      <.button>Send!</.button>
      <.button phx-click="go" variant="primary">Send!</.button>
      <.button variant="outline" navigate={~p"/"}>Home</.button>
  """
  attr :type, :string, default: nil
  attr :class, :any, default: nil

  attr :variant, :string,
    values: ~w(default primary secondary destructive outline ghost link),
    default: "default"

  attr :size, :string, values: ~w(default sm lg icon), default: "default"
  attr :rest, :global, include: ~w(href navigate patch method download name value disabled form)
  slot :inner_block, required: true

  def button(%{rest: rest} = assigns) do
    variant = if assigns.variant == "primary", do: "default", else: assigns.variant
    assigns = assign(assigns, :variant, variant)

    if rest[:href] || rest[:navigate] || rest[:patch] do
      assigns = assign(assigns, :variant_class, SaladUI.Helpers.button_variant(assigns))

      ~H"""
      <.link class={TwMerge.merge([@variant_class, @class])} {@rest}>
        {render_slot(@inner_block)}
      </.link>
      """
    else
      SaladUI.Button.button(assigns)
    end
  end

  @doc """
  Renders an input with label and error messages using SaladUI components.

  A `Phoenix.HTML.FormField` may be passed as argument,
  which is used to retrieve the input name, id, and values.
  Otherwise all attributes may be passed explicitly.
  """
  attr :id, :any, default: nil
  attr :name, :any
  attr :label, :string, default: nil
  attr :value, :any

  attr :type, :string,
    default: "text",
    values: ~w(checkbox color date datetime-local email file month number password
               search select tel text textarea time url week hidden)

  attr :field, Phoenix.HTML.FormField,
    doc: "a form field struct retrieved from the form, for example: @form[:email]"

  attr :errors, :list, default: []
  attr :checked, :boolean, doc: "the checked flag for checkbox inputs"
  attr :prompt, :string, default: nil, doc: "the prompt for select inputs"
  attr :options, :list, doc: "the options to pass to Phoenix.HTML.Form.options_for_select/2"
  attr :multiple, :boolean, default: false, doc: "the multiple flag for select inputs"
  attr :class, :any, default: nil, doc: "the input class to use over defaults"
  attr :error_class, :any, default: nil, doc: "the input error class to use over defaults"

  attr :rest, :global,
    include: ~w(accept autocomplete capture cols disabled form list max maxlength min minlength
                multiple pattern placeholder readonly required rows size step)

  def input(%{field: %Phoenix.HTML.FormField{} = field} = assigns) do
    errors = if Phoenix.Component.used_input?(field), do: field.errors, else: []

    assigns
    |> assign(field: nil, id: assigns.id || field.id)
    |> assign(:errors, Enum.map(errors, &translate_error(&1)))
    |> assign_new(:name, fn -> if assigns.multiple, do: field.name <> "[]", else: field.name end)
    |> assign_new(:value, fn -> field.value end)
    |> input()
  end

  def input(%{type: "hidden"} = assigns) do
    ~H"""
    <SaladUI.Input.input type="hidden" id={@id} name={@name} value={@value} {@rest} />
    """
  end

  def input(%{type: "checkbox"} = assigns) do
    assigns =
      assign_new(assigns, :checked, fn ->
        Phoenix.HTML.Form.normalize_value("checkbox", assigns[:value])
      end)

    ~H"""
    <SaladUI.Form.form_item class="mb-2">
      <div class="flex items-center gap-2">
        <SaladUI.Checkbox.checkbox
          id={@id}
          name={@name}
          value={@checked}
          class={@class}
          {@rest}
        />
        <SaladUI.Label.label :if={@label} for={@id}>{@label}</SaladUI.Label.label>
      </div>
      <.error :for={msg <- @errors}>{msg}</.error>
    </SaladUI.Form.form_item>
    """
  end

  def input(%{type: "select"} = assigns) do
    ~H"""
    <SaladUI.Form.form_item class="space-y-1.5">
      <SaladUI.Label.label :if={@label} for={@id}>{@label}</SaladUI.Label.label>
      <select
        id={@id}
        name={@name}
        class={
          TwMerge.merge([
            "flex h-10 w-full items-center justify-between rounded-md border border-input bg-background px-3 py-2 text-sm focus-visible:outline-hidden focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50",
            @class,
            @errors != [] && (@error_class || "border-destructive")
          ])
        }
        multiple={@multiple}
        {@rest}
      >
        <option :if={@prompt} value="">{@prompt}</option>
        {Phoenix.HTML.Form.options_for_select(@options, @value)}
      </select>
      <.error :for={msg <- @errors}>{msg}</.error>
    </SaladUI.Form.form_item>
    """
  end

  def input(%{type: "textarea"} = assigns) do
    ~H"""
    <SaladUI.Form.form_item class="space-y-1.5">
      <SaladUI.Label.label :if={@label} for={@id}>{@label}</SaladUI.Label.label>
      <SaladUI.Textarea.textarea
        id={@id}
        name={@name}
        value={@value}
        class={[@class, @errors != [] && (@error_class || "border-destructive")]}
        {@rest}
      />
      <.error :for={msg <- @errors}>{msg}</.error>
    </SaladUI.Form.form_item>
    """
  end

  # All other inputs text, datetime-local, url, password, etc. are handled here...
  def input(assigns) do
    ~H"""
    <SaladUI.Form.form_item class="space-y-1.5">
      <SaladUI.Label.label :if={@label} for={@id}>{@label}</SaladUI.Label.label>
      <SaladUI.Input.input
        type={@type}
        name={@name}
        id={@id}
        value={Phoenix.HTML.Form.normalize_value(@type, @value)}
        class={[@class, @errors != [] && (@error_class || "border-destructive")]}
        {@rest}
      />
      <.error :for={msg <- @errors}>{msg}</.error>
    </SaladUI.Form.form_item>
    """
  end

  # Helper used by inputs to generate form errors
  defp error(assigns) do
    ~H"""
    <p class="mt-1.5 flex items-center gap-2 text-sm text-destructive">
      <.icon name="hero-exclamation-circle" class="size-5" />
      {render_slot(@inner_block)}
    </p>
    """
  end

  @doc """
  Renders a header with title.
  """
  slot :inner_block, required: true
  slot :subtitle
  slot :actions

  def header(assigns) do
    ~H"""
    <header class={[@actions != [] && "flex items-center justify-between gap-6", "pb-4"]}>
      <div>
        <h1 class="text-2xl font-semibold tracking-tight text-foreground">
          {render_slot(@inner_block)}
        </h1>
        <p :if={@subtitle != []} class="mt-1 text-sm text-muted-foreground">
          {render_slot(@subtitle)}
        </p>
      </div>
      <div class="flex-none">{render_slot(@actions)}</div>
    </header>
    """
  end

  @doc """
  Renders a data list inside a SaladUI card.

  ## Examples

      <.list>
        <:item title="Title">{@post.title}</:item>
        <:item title="Views">{@post.views}</:item>
      </.list>
  """
  slot :item, required: true do
    attr :title, :string, required: true
  end

  def list(assigns) do
    ~H"""
    <SaladUI.Card.card>
      <dl class="divide-y divide-border">
        <div
          :for={item <- @item}
          class="grid gap-1 px-6 py-4 text-sm sm:grid-cols-3 sm:gap-4"
        >
          <dt class="font-medium text-muted-foreground">{item.title}</dt>
          <dd class="text-foreground sm:col-span-2">{render_slot(item)}</dd>
        </div>
      </dl>
    </SaladUI.Card.card>
    """
  end

  @doc """
  Renders a [Heroicon](https://heroicons.com) via `SaladUI.Icon`.

  ## Examples

      <.icon name="hero-x-mark" />
      <.icon name="hero-arrow-path" class="ml-1 size-3 motion-safe:animate-spin" />
  """
  attr :name, :string, required: true
  attr :class, :string, default: "size-4"

  def icon(%{name: "hero-" <> _} = assigns) do
    SaladUI.Icon.icon(assigns)
  end

  ## JS Commands

  def show(js \\ %JS{}, selector) do
    JS.show(js,
      to: selector,
      time: 300,
      transition:
        {"transition-all ease-out duration-300",
         "opacity-0 translate-y-4 sm:translate-y-0 sm:scale-95",
         "opacity-100 translate-y-0 sm:scale-100"}
    )
  end

  def hide(js \\ %JS{}, selector) do
    JS.hide(js,
      to: selector,
      time: 200,
      transition:
        {"transition-all ease-in duration-200", "opacity-100 translate-y-0 sm:scale-100",
         "opacity-0 translate-y-4 sm:translate-y-0 sm:scale-95"}
    )
  end

  @doc """
  Translates an error message using gettext.
  """
  def translate_error({msg, opts}) do
    if count = opts[:count] do
      Gettext.dngettext(LeashCollectorWeb.Gettext, "errors", msg, msg, count, opts)
    else
      Gettext.dgettext(LeashCollectorWeb.Gettext, "errors", msg, opts)
    end
  end

  @doc """
  Translates the errors for a field from a keyword list of errors.
  """
  def translate_errors(errors, field) when is_list(errors) do
    for {^field, {msg, opts}} <- errors, do: translate_error({msg, opts})
  end
end
