# PQC engine deployment

This directory keeps the Linux build notes for the engine and the application:

- [Linux component builds](BUILDING_LINUX.md)
- [Selected Rust consensus application](BUILDING_PQC_APPLICATION.md)

The native supervisor (`crates/native-supervisor`) owns the node service in
both builds: the disposable development mode, and the production mode of a
production build (production activation v1, A5). The policy renderer
(`tools/native-execution-policy`) renders its unit properties, AppArmor
profiles and, for a production node, its host firewall rules.

The Python supervisor (`supervise.py`), its systemd unit and its manifest
templates were retired in A5. They could not start the owner-guarded engine
and application, and no CI step ran them. They remain in git history.
