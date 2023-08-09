# ssl-ifier

Turns any http service into a full https service

## How to use
Run the program once to generate a `config.toml` in the exe directory, or fill in the following template and save as `config.toml` beside the exe
```toml
[addresses]
backend = "backendhostname:5000"
proxy = "backendhostname:443"
ssl_cert = "my.crt.pem"
ssl_key = "my.key.pem"
```
... That's it!

You may use an ip or hostname which resolves to an ip (if using for localhost serivces, you can add them in your hosts file).
