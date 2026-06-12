# Description:
WinlatorXR has its own UDP XrAPI protocol to communicate between the real Android based standalone VR device (likely a Meta Quest 2, 3, 3S, or Pico 4 Ultra), and the Windows emulated environment within Winlator which runs on Box86, Wine, Proton, and DXVK

The basic functionality of WinlatorXR's XrAPI is to use UDP to receive the 6DOF input and tracking data inside the Winlator container for the purpose of simulating a full VR experience, and sending UDP back to the host Android device to set variables controlling the side-by-side rendering (SBS), rendering a monocular viewport, or rendering stereo using alternate eye rendering (AER), as well as FOV and haptics

As of Version 0.4 of the XrAPI, there is two UDP ports used for receiving, so that two applications can run at the same time using tracking data (eg: OpentrackWXR and another app)

# Online Documentation:

Some basic documentation of the XrAPI exists online at https://winlatorxr.github.io/xrapi.html

# Terminology and Understanding This Document:

This is written from the perspective of the application running within a Winlator container

Received UDP is referred to as UDP Rx
Sent UDP is referred to as UDP Tx
Device Rotation data is sent as Quaternions (X,Y,Z,W)
Device Positional data is sent as Vectors (X,Y,Z)

Each device is tracked independently, relative to what is assumed to be "room center" on X and Z, and default user preferred height on Y. It is not relative to the HMD.

It does not contain acceleration data, that must be calculated on the WinlatorXR XrAPI implementation side if it's required

# UDP Receive Data (Within the Winlator container)

UDP data is received as a CSV string containing the XR 6DOF tracking data for the left controller (LHAND), right controller (RHAND) and worn VR device itself (HMD), as well as the input data for the controllers, the IPD and FOV data, and as of XrAPI V0.3 also the SBS and Immersive statuses

### UDP Rx IP:

127.0.0.1 (or localhost)

### UDP Rx Ports:
UDP Rx Port: 7872
Fallback UDP Rx Port: 7873

If it fails to load on the main Rx Port, silently try the Fallback before throwing an exception

### UDP Rx Format:

CLIENT,LHANDQX,LHANDQY,LHANDQZ,LHANDQW,LTHUMBX,LTHUMBY,LHANDX,LHANDY,LHANDZ,RHANDQX,RHANDQY,RHANDQZ,RHANDQW,RTHUMBX,RTHUMBY,RHANDX,RHANDY,RHANDZ,HMDQX,HMDQY,HMDQZ,HMDQW,HMDX,HMDY,HMDZ,IPD,FOVH,FOVV,CURRFRAMEID,BUTTONBOOLSTR,IMMERSIVESBSBOOLSTR

Description:

Client is ignored (usually client0)
Left Hand Quaternion X (float)
Left Hand Quaternion Y (float)
Left Hand Quaternion Z (float)
Left Hand Quaternion W (float)
Left Hand Thumbstick X (float)
Left Hand Thumbstick Y (float)
Left Hand Position X (float)
Left Hand Position Y (float)
Left Hand Position Z (float)
Right Hand Quaternion X (float)
Right Hand Quaternion Y (float)
Right Hand Quaternion Z (float)
Right Hand Quaternion W (float)
Right Hand Thumbstick X (float)
Right Hand Thumbstick Y (float)
Right Hand Position X (float)
Right Hand Position Y (float)
Right Hand Position Z (float)
HMD Quaternion X (float)
HMD Quaternion Y (float)
HMD Quaternion Z (float)
HMD Quaternion W (float)
HMD Position X (float)
HMD Position Y (float)
HMD Position Z (float)
IPD Value (float)
FOV Horizontal (float)
FOV Vertical (float)
Current OpenXR Frame ID (int)
Button Boolean String containing an T or F value for:
    -> Left Grip
    -> Left Menu
    -> Left Thumbstick Click
    -> Left Thumbstick Left
    -> Left Thumbstick Right
    -> Left Thumbstick Up
    -> Left Thumbstick Down
    -> Left Trigger
    -> Left Button X
    -> Left Button Y
    -> Right Button A
    -> Right Button B
    -> Right Grip
    -> Right Thumbstick Click
    -> Right Thumbstick Left
    -> Right Thumbstick Right
    -> Right Thumbstick Up
    -> Right Thumbstick Down
    -> Right Trigger
Immersive and SBS flag String containing a T or F value for:
    -> Is Immersive (is immersive mode enabled?)
    -> Is SBS (is side-by-side rendering active?)

The immersive and SBS flag string only exists in XrAPI 0.3 and newer

The right menu button is notably missing, this is because it's a WinlatorXR special input mapped for its own in-app pop-up configuration menu

### UDP Rx Example:

client0 0.210 0.290 -0.930 0.040 0.0 0.0 -0.008 -0.229 -0.173 0.100 -0.300 0.950 -0.080 0.0 0.0 0.154 -0.240 -0.140 0.150 -0.070 0.050 0.990 0.037 0.006 -0.017 0.0678 99.00 103.40 45 FFFFFFFFFFFFFFFFFFF FF

### UDP Rx Background Thread:

UDP Rx data must be received at all times without blocking another application from running, so it usually runs in a background thread that opens and closes with the application (eg: a game, a mod, a tool, or a compatibility layer)

In most use cases, the OpenXRFrameID FrameSync will block rendering in the target application until a new ID comes from the UDP Rx Data, that is expected

# UDP Send Data (To the VR device)

UDP data is sent to the Android device as a CSV string in order to both start the XrAPI up, and to control certain optional settings like whether the app should render in 3D SBS, 3D AER, or monocular. It also sends non-negative haptic data float value for left and right controllers, which will automatically reduce to 0 on the XrAPI side at a steady rate, so only send a value when a new vibration intensity is required

### UDP Tx IP:

127.0.0.1 (or localhost)

### UDP Tx Port:

UDP Tx Port: 7278

### UDP Tx Format:

L_VIBE,R_VIBE,VR,SBS,FOV_W,FOV_H

Description:

Haptic Vibration Strength Left Controller (float, default 0)
Haptic Vibration Strength Right Controller (float, default 0)
VR Flag (int, 0 for non-VR fullscreen, 1 for VR immersive mode, 2 for non-VR without head mouse control, 3 for non-VR with head mapped mouse control)
SBS Flag (int, 0 for monocular VR, 1 for SBS 3D, 2 for AER 3D)
Target FOV W (float, default 104.5)
Target FOV H (float, default 104.5)

### UDP Tx Example:

0 0 1 0 104.5 104.5

### UDP Tx Startup:

Send at least one single UDP packet to WinlatorXR at the start of the program to ensure it begins sending the UDP data we require back to us

# Future API features

This document was last modified in June 2026

The XrAPI is growing with each new release, check this document frequently for any updated versions of the XrAPI, new versions of XrAPI aim to be backwards compatible where possible, but if not then you may use an older version of the XrAPI to maintain functionality, but this may impact any other apps that also want to use XrAPI at the same time