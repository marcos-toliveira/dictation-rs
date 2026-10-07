# Empacotamento (Arch Linux / AUR)

Pacote **AUR**: `dictation-rs` — compila do código-fonte (tag de release) e instala
`dictationd` + `dictation` em `/usr/bin`.

## Instalar localmente (a partir desta pasta)

```bash
cd packaging
makepkg -si          # compila, empacota e instala via pacman
```

O `makepkg` roda dentro do diretório; ele baixa a tarball da tag `v0.1.0`, compila com
`cargo --release` e instala. Ao final, o `.install` mostra os passos por usuário
(cofre do Groq, config, `systemctl --user enable --now dictationd.service`, atalho).

## Como publicar no AUR (para `yay`/`paru` instalarem)

O **AUR** é um repositório de *PKGBUILDs* mantido pela comunidade — é o caminho natural
(pacotes oficiais do `pacman` exigem ser *Package Maintainer*/TU, o que não se aplica a
uma ferramenta pessoal).

1. Crie a conta em <https://aur.archlinux.org> e adicione sua **chave SSH pública**
   em *My Account*.
2. Clone o repositório do pacote (cria o repo no AUR no primeiro push):

   ```bash
   git clone ssh://aur@aur.archlinux.org/dictation-rs.git
   cd dictation-rs
   ```

3. Copie para lá o `PKGBUILD` e o `dictation-rs.install`, e gere o `.SRCINFO`:

   ```bash
   cp ../packaging/PKGBUILD ../packaging/dictation-rs.install .
   makepkg --printsrcinfo > .SRCINFO
   ```

4. Publique:

   ```bash
   git add PKGBUILD dictation-rs.install .SRCINFO
   git commit -m "Initial import: dictation-rs 0.1.0"
   git push
   ```

Depois disso, qualquer usuário Arch instala com:

```bash
yay -S dictation-rs      # ou: paru -S dictation-rs
```

## Atualizar a cada nova versão

1. Crie a tag no GitHub (`vX.Y.Z`) e ajuste `pkgver` no `PKGBUILD`.
2. Atualize os checksums:

   ```bash
   updpkgsums           # recalcula sha256sums a partir do source
   makepkg --printsrcinfo > .SRCINFO
   git commit -am "upgpkg: dictation-rs X.Y.Z-1" && git push
   ```

## Alternativa: pacote de desenvolvimento (`-git`)

Para acompanhar a `main` em vez de tags, publique um segundo pacote `dictation-rs-git`
com `source=("$pkgname::git+$url.git")` e `pkgver()` via `git describe`. Não é obrigatório.

## Alternativa: repositório pacman próprio

Também é possível (não precisa do AUR) gerar o `.pkg.tar.zst` com `makepkg`, rodar
`repo-add dictation-rs.db.tar.gz *.pkg.tar.zst` e hospedar o repo; o usuário adiciona a
linha `[meu-repo]` em `/etc/pacman.conf`. Útil para distribuição interna.
