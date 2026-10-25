// leash-proxy default policy set.
//
// These are examples: deployments are expected to replace or extend them.
// Cedar denies by default, so anything not permitted below is blocked.

// Read-only GitHub API access is granted to every agent.
permit(principal, action == Action::"http_get", resource == Host::"api.github.com");

// The build agent may install python packages from PyPI.
permit(
    principal == Agent::"vm-build",
    action == Action::"http_get",
    resource == Host::"pypi.org"
)
when { context.path like "/simple/*" };

// The internal admin console is forbidden outright, even if other policies
// are later extended to allow it.
forbid(principal, action, resource == Host::"admin.internal");
