# Release with version bump
release version:
    cargo set-version {{version}}
    git add Cargo.toml Cargo.lock
    git commit -m "chore: bump version to {{version}}"
    git tag v{{version}}
    git push origin master
    git push origin v{{version}}

# Release patch version (0.1.0 -> 0.1.1)
release-patch:
    #!/usr/bin/env bash
    NEW_VERSION=$(cargo pkgid | cut -d# -f2 | cut -d@ -f2 | awk -F. '{$3=$3+1; print $1"."$2"."$3}')
    just release $NEW_VERSION

# Release minor version (0.1.0 -> 0.2.0)
release-minor:
    #!/usr/bin/env bash
    NEW_VERSION=$(cargo pkgid | cut -d# -f2 | cut -d@ -f2 | awk -F. '{$2=$2+1; $3=0; print $1"."$2"."$3}')
    just release $NEW_VERSION

# Release major version (0.1.0 -> 1.0.0)
release-major:
    #!/usr/bin/env bash
    NEW_VERSION=$(cargo pkgid | cut -d# -f2 | cut -d@ -f2 | awk -F. '{$1=$1+1; $2=0; $3=0; print $1"."$2"."$3}')
    just release $NEW_VERSION