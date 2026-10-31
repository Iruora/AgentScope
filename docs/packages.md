# Package managers

The [Packages workflow](../.github/workflows/packages.yml) publishes each
release to Homebrew, Scoop, winget and the AUR. The Release workflow runs it
right after it publishes a release. You can also run it from the Actions tab
with a released tag, for example to retry one package manager.

Each package manager stays off until its secret exists, so releases work
without any of this. Set up only the ones you want.

## Homebrew and Scoop

1. Create two empty public repositories: `Iruora/homebrew-agentscope` and
   `Iruora/scoop-agentscope`. Tick "Add a README" so each has a first commit.
2. Create a fine-grained personal access token with **Contents: Read and
   write** on those two repositories only.
3. Add it as the repository secret `PACKAGES_TOKEN` on AgentScope.

People then install with:

```sh
brew install --cask iruora/agentscope/agentscope
scoop bucket add agentscope https://github.com/Iruora/scoop-agentscope
scoop install agentscope
```

## winget

1. Create a classic personal access token with the `public_repo` scope.
2. Add it as the repository secret `WINGET_TOKEN`.

Each release opens a pull request on `microsoft/winget-pkgs` from a fork
under your account. The first one is reviewed by the winget maintainers, which
can take a few days; later versions are usually merged automatically. Then:
`winget install Iruora.AgentScope`.

## AUR (Arch Linux)

1. Create an account on [aur.archlinux.org](https://aur.archlinux.org) and add
   an SSH public key to it.
2. Add the matching private key as the repository secret `AUR_SSH_PRIVATE_KEY`.

The first release creates the `agentscope-bin` package. Then:
`yay -S agentscope-bin` (or any AUR helper).

## Flathub

Not automated yet: Flathub needs a one-time submission and builds from
source on its own infrastructure.
