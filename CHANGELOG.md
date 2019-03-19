# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `!song promote <number>` to promote songs to the top of the queue ([#2]).
- Optional web page (hosted on `setbac.tv`) to display current queue ([#3]) (config: `api_url`).
- Support suppressing echoing of current song (config: `[player] echo_current_song = false`).
- Show queue position in player view ([#8])
- Support deleting a song at the given position ([#7])

### Changed
- Changed configuration format to be TOML and flatten it (see [example configuration]).
- Removed HTML escapes from `current_song` ([#4])
- Fixed `!song purge` not sending update to `setbac.tv` ([#9])

[example configuration]: https://github.com/udoprog/setmod/blob/master/config.toml
[#2]: https://github.com/udoprog/setmod/issues/2
[#3]: https://github.com/udoprog/setmod/issues/3
[#4]: https://github.com/udoprog/setmod/issues/4
[#7]: https://github.com/udoprog/setmod/issues/7
[#8]: https://github.com/udoprog/setmod/issues/8
[#9]: https://github.com/udoprog/setmod/issues/9

[Unreleased]: https://github.com/udoprog/setmod/compare/0.0.1...HEAD