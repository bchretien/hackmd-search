hackmd-search
=============

This tool allows to retrieve the [HackMD](https://hackmd.io) content of a team and make it searchable with [Meilisearch](https://www.meilisearch.com).

## Build

```
$ cargo build --release
```

## Usage

For now the tool operates in 2 steps:
- Download the HackMD content of your team,
- Index the pages with Meilisearch to make them available.

To retrieve the HackMD content:
```
$ hackmd-search --update --team <TEAM NAME> --database <PATH TO THE JSON DATABASE>
```
This will prompt for your HackMD user (e-mail) and password, download
everything, and store it in a JSON file for reuse.

You can then send this data to Meilisearch for quick searches:
```
$ hackmd-search --database <PATH TO THE JSON DATABASE> --meilisearch <MEILISEARCH URL> 
```

Note that you can do these 2 steps together:
```
$ hackmd-search --update --team <TEAM NAME> --database <PATH TO THE JSON DATABASE> --meilisearch <MEILISEARCH URL>
```

To get the usage:
```
$ hackmd-search --help
```

## Start Meilisearch

The easiest option is to use Docker:
```
$ docker run -p 7700:7700 -v "$(pwd)/data.ms:/data.ms" getmeili/meilisearch ./meilisearch --no-analytics
```

Then when you run `hackmd-search`, use the `--meilisearch http://localhost:7700` argument.

## License

[GNU GPLv3](COPYING).
