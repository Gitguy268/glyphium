# Flatpak

A self-hosted Flatpak of Glyphium. It isn't on Flathub. Releases carry an
`io.github.gitguy268.glyphium.flatpak` file built by `.github/workflows/release.yml`.

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user ./io.github.gitguy268.glyphium.flatpak
flatpak run io.github.gitguy268.glyphium
```

Your login shell runs on the host through [host-spawn](https://github.com/1player/host-spawn)
(MIT-0), so the sandbox needs `--talk-name=org.freedesktop.Flatpak`. `glyphium -e COMMAND` runs a
command inside the sandbox instead.

The Linux build is new and only checked in CI. "Queue next" waits for the shell to go idle, and
with host-spawn in between that detection may not work yet.

Build it yourself on Linux (x86_64), from the repo root, after the Linux build steps in
`.github/workflows/release.yml`:

```sh
cp target/release/glyphium packaging/flatpak/glyphium-linux-x86_64
cd packaging/flatpak
flatpak-builder --user --install-deps-from=flathub --force-clean --repo=repo build io.github.gitguy268.glyphium.yml
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo repo glyphium.flatpak io.github.gitguy268.glyphium
```
