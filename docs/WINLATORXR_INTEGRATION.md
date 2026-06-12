# Description

WinlatorXR is an app that runs on a real Android based standalone VR device (likely a Meta Quest 2, 3, 3S, or Pico 4 Ultra) to emulate Windows software applications via compatibility layers like Box86, Wine, Proton, and DXVK

It was originally designed purely for flat non-VR experiences, but began experimenting with experimental 3DOF injection via attaching the screen to the users face and using the head angle to map mouse movements

It has now evolved to a level where it has its own "XrAPI" used in projects like SixDOFinator Sample Project, Halo CE WinlatorXR (HWXR), and OpentrackWXR for example, and continues to grow

The basic functionality of WinlatorXR's XrAPI is to use UDP to receive the 6DOF input and tracking data inside the Winlator container for the purpose of simulating a full VR experience, and sending UDP back to the host Android device to set variables controlling the side-by-side rendering (SBS), rendering a monocular viewport, or rendering stereo using alternate eye rendering (AER), as well as FOV and haptics

The user has total control on the Winlator container and shortcut setup screen to determine their DXVK version (or system/Turnip drivers) and container resolution as well, we usually advise users set 1400x1400 for immersive VR experiences using XrAPI as that worked best in our testing

Video modes currently supported are:

1. Monocular VR (single eye viewport), many users find this the most comfortable, and it is more performant for the standalone hardware to run

2. Side-By-Side VR (SBS), this mode renders in true 3D, but results in half the horizontal resolution to fit two frames in the same screen

3. Alternate Eye Rendering VR (AER), this is a mode used by a lot of popular mods of games as it allows the 3D effect to exist without needing to re-write the entire camera system. It's also more performant than true SBS 3D because it only renders one frame at once, rather than two

Ideally every XrAPI application should offer monocular mode, and then optionally SBS or AER for "VR purists" who want that stereo depth and refuse to part with it, no matter the negative impacts it has on performance and visual quality

Some applications use the 6DOF pose data without actually changing standard WinlatorXR behavior like OpentrackWXR, which passes head tracking data to the "opentrack" system but still lets users freely switch the immersive and SBS modes via controller inputs and use the right controller as a mouse (in both windowed and immersive mode). These use some very loosely documented versions of the VR and SBS flags in UDP transmit which are not commonly used

See PROTOCOL.md in /docs for more about the UDP formatting of XrAPI

### Documentation:

WinlatorXR About Page: https://winlatorxr.github.io/about.html

WinlatorXR XrAPI Page: https://winlatorxr.github.io/xrapi.html

### Example Projects

SixDOFinator Sample Project, Unity3D Game, C#: https://github.com/WinlatorXR/SixDOFinator_SampleProject

OpentrackWXR, QT UI, C++: https://github.com/WinlatorXR/opentrackWXR

HaloCEWXR, DLL game mod for Halo Combat Evolved, C++: https://github.com/WinlatorXR/HaloCEWXR

### Testing Tool

There is a tool to simulate the XR tracking data on Windows that will not usually be necesssary for AI to use, but is available at: https://github.com/WinlatorXR/XrAPIUDPTester

# Starting a WinlatorXR XrAPI project

Most projects that use the WinlatorXR XrAPI are not built from scratch like SixDOFinator was, instead they are modifications to existing software that either supports SteamVR's OpenVR, or some other OpenXR integration

Most of the time these existing integrations should be completely removed for a fresh fork made specifically for WinlatorXR, as their removal both improves the codebase for our specific purposes, and also is likely to remove overhead that could slow down the experience on limited standalone hardware

### Start-up process 

Starting up the WinlatorXR XrAPI project, we will immediately want to create a file in "Z:/tmp/xr" called "version" and write in the highest value currently available to the public (right now 0.4) inside it. If it already exists, we update it to the newest version available still so our app works as intended, it should not break any other app that might be using it already

Older versions of the XrAPI also create a "vr" file in this directory when intending to support the full VR native rendering mode, this is not necessary

We then start the background thread UDP receiver and send a single transmit UDP packet to start the XrAPI on the Android side

See PROTOCOL.md in /docs for more about the UDP formatting of XrAPI

### Shut-down process

We do our best to ensure the UDP Rx background thread is shutdown, this is ideal but not entirely a problem if it doesn't work, as the whole container is likely to be closed at the same time as this project

Clean exiting is preferred simply in case a user is using the WinlatorXR container in desktop mode, rather than launching from a per-app shortcut which fully closes the container on exit

We delete the "vr" file in the "Z:/tmp/xr" directory when closing if it exists, but not the "version" file in case something else is using it

### Handling "upside down hands"

A known issue of certain devices like the Meta Quest 2 and Pico 4 Ultra is that somehow their hand rotation is upside-down compared to the Meta Quest 3

To fix this, we often detect the model of the device from data provided by WinlatorXR in the "Z:/tmp/xr" directory in a file named "system"

That file contains the first string of HMD Make, and second string of HMD Model

if the make is "META" but the model is not "EUREKA" or "PANTHER" (Meta Quest 3 and 3 S) then we assume it's a Quest 2 and toggle the upside down hands fix. This might be a bit short-sighted for future META HMDs but that is something we can adjust in the future if need be too

However, often users might want an option to manually toggle this upside down hands fix, usually by placing a file in the game or mod directory called "handsfix.txt"

### The OpenXR Frame ID Value and "FrameSync"

WinlatorXR uses a small (5px by 5px on average) square rendered in the top-left corner of the application to synchronize the OpenXR pose data with the rendered frame for smoother experiences to end-users

This visual data is the only way to communicate the correct pose ID back to Android from Winlator / Wine reliably as both are running asynchronously

This value is an INT between 0 and 255, and constantly continues to cycle 0 to 255 on the Android VR side

This maps to an RGB value red channel (eg: (0, 0, 0) to (255, 0 0) colors) for the square, sometimes the value must be adjusted to properly display in SRGB color space if the game or app or mod is using something else. This is best left to a human eye to determine if it's correct

It is best to hold the current rendered frame in the application or game until a newer OpenXR Frame ID is received than the last UDP packet (anything other than the last Frame ID received)

This is easier to do in games made from scratch in an engine like Unity3D, where you can use a simple "While" loop to check if the new frame ID has been changed by the background UDP thread

Otherwise, a clever method is needed to pause the game loop for a given game (eg: pausing the 6DOF tracking data usage even if the game still renders in realtime)

For mods of games, there is no universal method to inject the red square data and they must be tailored to each game

This is an optional feature for user comfort, if it cannot be done at all, it can be skipped for a lower quality experience

This is not necessary at all for applications that aren't using the VR "native rendering" mode of XrAPI (eg: OpentrackWXR)

### Alternate Eye Rendering

Rather than monocular VR or SBS 3D, there is an Alternate Eye Rendering option that is available too, where the BLUE channel of the OepnXR Frame ID tells the Android side which eye that frame belongs to (0 for left, 255 for right)

AER is used in SixDOFinator and HWXR (Halo CE WXR) currently