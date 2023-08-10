# ssl-ifier

Turns any http service into a full https service

## How to use
Run the program once to generate a `config.toml` in the exe directory, or fill in the following template and save as `config.toml` beside the exe
```toml
[addresses]
backend = "backendhostname:5000"
proxy = "proxyhostname:443"
ssl_cert = "my.crt.pem"
ssl_key = "my.key.pem"
```
... That's it!

You may use an ip or hostname which resolves to an ip (if using for localhost serivces, you can add them in your hosts file).

The certificate and key must both be in pem format. If you need help making a CA / ssl certificates for yourself, see [this stackoverflow answer](https://stackoverflow.com/a/60516812/9423933). (To convert to pem, use `openssl x509 -in mycert.crt -out mycert.crt.pem` and `openssl rsa -in mycert.key -out mycert.key.pem`)
