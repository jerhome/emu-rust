#!/usr/bin/env python3
"""Injects keyboard / pointer events into an X display through XTEST (no xdotool in the dev container).

usage: xinject.py DISPLAY ACTION...
  focus              give the input focus to the largest mapped top-level window and warp the pointer into it
  sleep SECONDS
  down KEYSYM | up KEYSYM | tap KEYSYM        e.g. Control_R, d, j, Escape
  move X Y | click BUTTON
Only talks to the X server it is told about; libX11 and libXtst are the only requirements.
"""
import ctypes
import ctypes.util
import sys
import time


def main(argv):
    display, actions = argv[1], argv[2:]
    x11 = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
    xtst = ctypes.CDLL(ctypes.util.find_library("Xtst") or "libXtst.so.6")
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XDefaultRootWindow.restype = ctypes.c_ulong
    x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    x11.XKeysymToKeycode.restype = ctypes.c_ubyte
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XStringToKeysym.restype = ctypes.c_ulong
    x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
    xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    dpy = x11.XOpenDisplay(display.encode())
    if not dpy:
        sys.exit(f"cannot open display {display}")
    root = x11.XDefaultRootWindow(dpy)

    def keycode(name):
        sym = x11.XStringToKeysym(name.encode())
        code = x11.XKeysymToKeycode(dpy, sym) if sym else 0
        if not code:
            sys.exit(f"unknown key {name}")
        return code

    def focus():
        # Walk the root's children: the app window is the only big mapped one (no window manager here).
        class Attr(ctypes.Structure):  # XWindowAttributes
            _fields_ = [("x", ctypes.c_int), ("y", ctypes.c_int), ("w", ctypes.c_int), ("h", ctypes.c_int),
                        ("border", ctypes.c_int), ("depth", ctypes.c_int), ("visual", ctypes.c_void_p),
                        ("root", ctypes.c_ulong), ("cls", ctypes.c_int), ("bit_gravity", ctypes.c_int),
                        ("win_gravity", ctypes.c_int), ("backing_store", ctypes.c_int),
                        ("backing_planes", ctypes.c_ulong), ("backing_pixel", ctypes.c_ulong),
                        ("save_under", ctypes.c_int), ("colormap", ctypes.c_ulong),
                        ("map_installed", ctypes.c_int), ("map", ctypes.c_int),
                        ("all_events", ctypes.c_long), ("your_events", ctypes.c_long),
                        ("no_propagate", ctypes.c_long), ("override", ctypes.c_int),
                        ("screen", ctypes.c_void_p)]

        r, p = ctypes.c_ulong(), ctypes.c_ulong()
        kids, n = ctypes.POINTER(ctypes.c_ulong)(), ctypes.c_uint()
        x11.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong),
                                   ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)),
                                   ctypes.POINTER(ctypes.c_uint)]
        x11.XGetWindowAttributes.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(Attr)]
        x11.XQueryTree(dpy, root, ctypes.byref(r), ctypes.byref(p), ctypes.byref(kids), ctypes.byref(n))
        best, area = None, 0
        for i in range(n.value):
            a = Attr()
            if x11.XGetWindowAttributes(dpy, kids[i], ctypes.byref(a)) and a.map == 2 and a.w * a.h > area:
                best, area, geo = kids[i], a.w * a.h, (a.x, a.y, a.w, a.h)
        if best is None:
            sys.exit("no mapped window to focus")
        x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
        x11.XSetInputFocus(dpy, best, 1, 0)  # RevertToPointerRoot, CurrentTime
        xtst.XTestFakeMotionEvent(dpy, -1, geo[0] + geo[2] // 2, geo[1] + geo[3] // 2, 0)

    i = 0
    while i < len(actions):
        a = actions[i]
        if a == "focus":
            focus()
            i += 1
        elif a == "sleep":
            x11.XFlush(dpy)
            time.sleep(float(actions[i + 1]))
            i += 2
        elif a in ("down", "up", "tap"):
            code = keycode(actions[i + 1])
            if a in ("down", "tap"):
                xtst.XTestFakeKeyEvent(dpy, code, 1, 0)
            if a == "tap":
                x11.XSync(dpy, 0)
                time.sleep(0.12)
            if a in ("up", "tap"):
                xtst.XTestFakeKeyEvent(dpy, code, 0, 0)
            x11.XSync(dpy, 0)
            time.sleep(0.12)
            i += 2
        elif a == "move":
            xtst.XTestFakeMotionEvent(dpy, -1, int(actions[i + 1]), int(actions[i + 2]), 0)
            x11.XSync(dpy, 0)
            i += 3
        elif a == "click":
            for press in (1, 0):
                xtst.XTestFakeButtonEvent(dpy, int(actions[i + 1]), press, 0)
                x11.XSync(dpy, 0)
                time.sleep(0.08)
            i += 2
        else:
            sys.exit(f"unknown action {a}")
    x11.XFlush(dpy)


if __name__ == "__main__":
    main(sys.argv)
