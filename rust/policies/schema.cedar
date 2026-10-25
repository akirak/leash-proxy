// leash-proxy Cedar authorization schema.
//
// Every client request is evaluated as a Cedar authorization request:
//
//   principal: Agent::<agent id>          (the micro VM the traffic came from)
//   action:    Action::"http_<verb>"      (e.g. "http_get", "http_connect")
//   resource:  Host::<destination host>   (attributes: port, scheme)
//   context:   { path }                   (path with query string)

entity Agent;

entity Host {
    port?: Long,
    scheme?: String,
};

action
    "http_connect",
    "http_delete",
    "http_get",
    "http_head",
    "http_options",
    "http_patch",
    "http_post",
    "http_put",
    "http_trace"
    appliesTo {
        principal: Agent,
        resource: Host,
        context: {
            path: String,
        },
    };
