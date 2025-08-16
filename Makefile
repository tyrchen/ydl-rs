build:
	@cargo build

test:
	@cargo nextest run --all-features

release:
	@cargo release tag --execute
	@git cliff -o CHANGELOG.md
	@git commit -a -n -m "Update CHANGELOG.md" || true
	@git push origin master
	@cargo release push --execute

publish:
	@cargo publish -p ydl
	@cargo publish -p ydl-cli

update-submodule:
	@git submodule update --init --recursive --remote

.PHONY: build test release update-submodule
