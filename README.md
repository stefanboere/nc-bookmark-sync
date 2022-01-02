Nextcloud bookmark sync
=======================

A bidirectional sync between Nextcloud Bookmarks and a plain text file.
The plain text file contains one bookmark per line in the following format:

```txt
path/to/bookmark https://www.example.com/
path/to/another_bookmark https://www.example.org/
```

This is the same format as the qutebrowser quickmarks.

Usage
-----

To perform the bidirectional sync, run

```sh
nc-bookmark-sync <path-to-configuration-file>.toml
```

The format of the configuration file is explained in the following section.
You should probably run this once in a while, e.g. as a systemd service.

Configuration file format
-------------------------

The .toml config file consists of three sections; `[general]`, `[pair]` and `[storage]`.
The storage section specified the two places where the bookmarks are stored,
e.g. the local storage or the Nextcloud Bookmarks app.
A pair connects two storages `a` and `b` and specifies a conflict_resolution.
See `examples/config.toml` for an example configuration file.

| Name  | Description | Example |
|-------|-------------|---------|
| `general.status_path` | Path where internal state is stored between runs | `$XDG_DATA_DIR/nc-bookmark-sync/status/` |
| `pair.a` | The name of the first storage | `cloud_example_com_local` |
| `pair.b` | The name of the second storage | `cloud_example_com_remote` |
| `pair.conflict_resolution` | How conflicts are used | `a wins`, `b wins` or `error` |
| `storage.type` | The type of storage | `nextcloud` or `file` |
| `storage.path` | The path to the bookmarks file (for type `file`) | `/home/john/.config/qutebrowser/quickmarks` |
| `storage.url`  | Rest API endpoint of Nextcloud Bookmarks | `https://cloud.example.com/index.php/apps/bookmarks/public/rest/v2` |
| `storage.username` | Nextcloud user name | A command, see Commands section |
| `storage.password` | Nextcloud password | A command, see Commands section |

Commands
--------

The storage username and passwords are the results of shell commands.
This allows them to be read from password storages, such as `pass`.
The format is as follows

```toml
[storage.<storage_name>.password]
fetch = ["command", "<executable>", "argument1", "argument2", "..."]
```

Nix module
----------

The `nc-bookmark-sync` module is a home-manager module which configures a
systemd service running `nc-bookmark-sync` at a certain interval.
The overlay should be added to add the `nc-bookmark-sync` executable to the
package sets.
The hmModule should be imported to install the home manager module.

Using `flake.nix`:

```nix
{
  inputs = {
    home-manager.url = "github:nix-community/home-manager";
    nc-bookmark-sync.url = "github:stefanboere/nc-bookmark-sync";
  };

  outputs = {self, nixpkgs, home-manager, nc-bookmark-sync, ...}: {
    overlays = [ nc-bookmark-sync.overlay ];
    nixosConfigurations.exampleHost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        home-manager.nixosModules.home-manager
        {
          home-manager.users.exampleUser = { pkgs, ... }: {
            imports = [ nc-bookmark-sync.hmModule ];
            services.nc-bookmark-sync = {
              enable = true;
              ...
            };
          };
        }
      ];
    };
  };
}

```

See for the configuration options the file `modules/nc-bookmark-sync.nix`.
