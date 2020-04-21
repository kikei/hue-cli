# hue-cli

Control Philips Hue from CLI

Note: Current implementation depends on customized version of [Orangenosecom/philipshue](https://github.com/Orangenosecom/philipshue). You need to clone it and fix Cargo.toml to get it enable to compile.

## Howto

At first, you can find your hue bridge by following command.
You will see IP address of that.

```sh
hue-cli discover
```

Then, register new API user to the bridge.
Typically you will be requested to push button on the bridge.

```sh
hue-cli register -b <ip-address-of-bridge> -d <device-type>
```

On registeration successful, long random string will be displayed, and it
is your username to access to apis.

To show all lights conected with the bridge, run light command.

```sh
hue-cli light -b <ip-address-of-bridge> -u <username>
```
