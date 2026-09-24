"""Finder layout for the Whisple disk image, based on Paper's Release page."""

application = defines["application"]
background = defines["background"]

format = "UDZO"
filesystem = "HFS+"
files = [application]
symlinks = {"Applications": "/Applications"}
hide_extensions = ["Whisple.app"]

window_rect = ((100, 100), (660, 400))
default_view = "icon-view"
show_toolbar = False
show_status_bar = False
show_pathbar = False
show_sidebar = False
show_icon_preview = False
icon_size = 128
text_size = 12
label_pos = "bottom"
icon_locations = {"Whisple.app": (180, 220), "Applications": (480, 220)}
