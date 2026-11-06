Name: dingotail
Version: @VERSION@
Release: 1%{?dist}
Summary: Native local and SSH log viewer
License: GPL-3.0-only
URL: https://github.com/jpdevoto/dingo-tail
ExclusiveArch: x86_64
Requires: libX11, libXcursor, libXi, libXrandr, libxkbcommon, libxkbcommon-x11
Requires: libwayland-client, libwayland-cursor, libwayland-egl, mesa-libEGL, mesa-libGL, xdg-desktop-portal
Recommends: gnome-keyring
Recommends: xdg-desktop-portal
Recommends: xdg-desktop-portal-gtk
%global debug_package %{nil}
%description
Read local files and Linux logs over SSH. Requires an OpenGL desktop.
Saved credentials require a running Secret Service.
%install
mkdir -p "%{buildroot}"
cp -a "@TREE@/." "%{buildroot}/"
%files
/usr/bin/dingotail
/usr/share/applications/dingotail.desktop
/usr/share/icons/hicolor/*/apps/dingotail.png
/usr/share/doc/dingotail
