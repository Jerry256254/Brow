# KucLab Brow - RPM spec (for rpmbuild on Fedora/RHEL/openSUSE).
# In this repo the .rpm is produced by packaging/mkrpm.py (no rpmbuild needed);
# this spec is the canonical recipe for distro builders.
Name:           kucclab-brow
Version:        0.1.0
Release:        1%{?dist}
Summary:        KucLab Brow - a small browser with its own engine (Brownie) and an ad blocker
License:        Apache-2.0
URL:            https://github.com/Jerry256254/Brow
Source0:        %{name}-%{version}.tar.gz
BuildArch:      x86_64
Requires:       glibc

%description
KucLab Brow renders every page with Brownie, its own HTML/CSS/layout/rasterizer
engine written in pure Rust (zero mandatory crates). Built-in ad blocker,
HTTP/1.1 + HTTPS (rustls), terminal UI. No bloat.

%prep
%setup -q

%build
cargo build --release -p brow --features tls

%install
install -Dpm 0755 target/release/brow %{buildroot}%{_bindir}/brow
install -Dpm 0644 README.md %{buildroot}%{_docdir}/%{name}/README.md
install -Dpm 0644 LICENSE %{buildroot}%{_docdir}/%{name}/LICENSE
install -Dpm 0644 filters/blocklist.txt %{buildroot}%{_datadir}/%{name}/blocklist.txt
install -Dpm 0644 packaging/kucclab-brow.desktop %{buildroot}%{_datadir}/applications/kucclab-brow.desktop

%files
%{_bindir}/brow
%{_docdir}/%{name}/README.md
%{_docdir}/%{name}/LICENSE
%{_datadir}/%{name}/blocklist.txt
%{_datadir}/applications/kucclab-brow.desktop

%changelog
* Thu Nov 26 2026 KucLab <dev@kucclab.local> - 0.1.0-1
- first KucLab Brow rpm: Brownie engine, terminal shell, ad blocker, HTTPS via rustls
