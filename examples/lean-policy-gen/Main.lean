/-
# Generate the example Cedar policy set in JSON (EST) format

Writes a Cedar JSON policy set equivalent of the default Cedar text policy set
in `rust/policies/policies.cedar`, so the two can be used interchangeably with
leash-proxy:

    lake build
    lake exe gen [output-file]

The generated file is a static policy set; pass its path to leash-proxy as a
policy file (`--policies`) and it is detected and loaded as JSON.
-/
import Policies

/-- The example policy set, mirroring `rust/policies/policies.cedar`. -/
def examplePolicies : List Policy :=
  [ -- Read-only GitHub API access is granted to every agent.
    { id := "github-readonly"
      action := httpGet
      resource := .eq "Host" "api.github.com" }
  , -- The build agent may install python packages from PyPI.
    { id := "pypi-simple"
      principal := .eq "Agent" "vm-build"
      action := httpGet
      resource := .eq "Host" "pypi.org"
      whens := [.like (.attr context "path") "/simple/*"] }
  , -- The internal admin console is forbidden outright, even if other
    -- policies are later extended to allow it.
    { id := "forbid-admin"
      effect := "forbid"
      resource := .eq "Host" "admin.internal" }
  ]

def main (args : List String) : IO UInt32 := do
  let out := args.headD "policies.json"
  IO.FS.writeFile out ((policySet examplePolicies).pretty ++ "\n")
  IO.println s!"wrote {out} ({examplePolicies.length} policies)"
  return 0
