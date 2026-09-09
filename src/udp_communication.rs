use std::io;
use std::net::Ipv4Addr;
use std::sync::Arc;

use glam::f32::{Quat, Vec2, Vec3};

pub struct UdpComm {
    receiver: socket2::Socket,
    transmitter: socket2::Socket,
    target_ip: Ipv4Addr,
}

#[derive(Clone)]
pub struct WinlatorPoseData {
    pub left_hand_quat: Quat,
    pub left_hand_thumb: Vec2,
    pub left_hand_pos: Vec3,
    pub right_hand_quat: Quat,
    pub right_hand_thumb: Vec2,
    pub right_hand_pos: Vec3,
    pub hmd_quat: Quat,
    pub hmd_pos: Vec3,
    pub ipd: f32,
    pub fov_h: f32,
    pub fov_v: f32,
    pub frame_id: u8,
    pub buttons: [bool; 19],
    pub immersive_mode: bool,
    pub sbs_mode: bool,
}

pub struct WinlatorHapticData {
    pub left_vibration: f32,
    pub right_vibration: f32,
    pub vr_flag: u8,
    pub sbs_flag: bool,
    pub target_fov_w: f32,
    pub target_fov_h: f32,
}

#[derive(Debug)]
pub enum UdpError {
    BindFailed(String),
    SendFailed(String),
    ReceiveFailed(String),
    ParseError(String),
}

impl std::fmt::Display for UdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UdpError::BindFailed(msg) => write!(f, "Failed to bind socket: {}", msg),
            UdpError::SendFailed(msg) => write!(f, "Failed to send: {}", msg),
            UdpError::ReceiveFailed(msg) => write!(f, "Failed to receive: {}", msg),
            UdpError::ParseError(msg) => write!(f, "Failed to parse: {}", msg),
        }
    }
}

impl std::error::Error for UdpError {}

/// Formats a `WinlatorHapticData` packet into the XrAPI UDP Tx CSV:
/// `L_VIBE,R_VIBE,VR,SBS,FOV_W,FOV_H` (space-separated, see PROTOCOL.md).
pub fn format_haptic_message(data: &WinlatorHapticData) -> String {
    let sbs_flag = if data.sbs_flag { "1" } else { "0" };
    format!(
        "{} {} {} {} {} {}",
        data.left_vibration,
        data.right_vibration,
        data.vr_flag,
        sbs_flag,
        data.target_fov_w,
        data.target_fov_h
    )
}

impl UdpComm {
    pub fn new() -> Result<Self, UdpError> {
        use socket2::{Socket, Domain, Type, Protocol};
        use std::net::{SocketAddrV4, Ipv4Addr};

        let target_ip = Ipv4Addr::new(127, 0, 0, 1);

        let receiver = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| UdpError::BindFailed(e.to_string()))?;
        // Non-blocking so the pose receiver thread can poll its shutdown flag
        // instead of blocking forever in `recv_from`.
        receiver
            .set_nonblocking(true)
            .map_err(|e| UdpError::BindFailed(e.to_string()))?;

        // Try the well-known Winlator ports first, then fall back to an
        // ephemeral port. This keeps parallel test runs (and multiple
        // instances) from failing to bind.
        let _receiver_port = match receiver.bind(&SocketAddrV4::new(target_ip, 7872).into()) {
            Ok(_) => 7872,
            Err(_) => match receiver.bind(&SocketAddrV4::new(target_ip, 7873).into()) {
                Ok(_) => 7873,
                Err(_) => {
                    receiver
                        .bind(&SocketAddrV4::new(target_ip, 0).into())
                        .map_err(|e| UdpError::BindFailed(e.to_string()))?;
                    0
                }
            },
        };

        let transmitter = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| UdpError::BindFailed(e.to_string()))?;

        transmitter.set_nonblocking(true)
            .map_err(|e| UdpError::BindFailed(e.to_string()))?;

        Ok(Self {
            receiver,
            transmitter,
            target_ip,
        })
    }

    pub fn send_haptic(&self, data: &WinlatorHapticData) -> Result<(), UdpError> {
        use socket2::Socket;
        use std::net::SocketAddrV4;

        let message = format_haptic_message(data);

        let target = SocketAddrV4::new(self.target_ip, 7278);
        self.transmitter.send_to(message.as_bytes(), &target.into())
            .map_err(|e| UdpError::SendFailed(e.to_string()))?;

        Ok(())
    }

    pub fn receive_pose(&self) -> Result<Option<String>, UdpError> {
        let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 4096];

        match self.receiver.recv_from(&mut buffer) {
            Ok((len, _addr)) => {
                if len > 0 {
                    let bytes = unsafe {
                        std::slice::from_raw_parts(buffer.as_ptr() as *const u8, len)
                    };
                    std::str::from_utf8(bytes)
                        .map(|s| Some(s.to_string()))
                        .map_err(|e| UdpError::ReceiveFailed(e.to_string()))
                } else {
                    Ok(None)
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                Ok(None)
            }
            Err(e) => {
                log::error!("Pose receive error: {:?}", e);
                Err(UdpError::ReceiveFailed(e.to_string()))
            }
        }
    }
}

pub fn parse_winlator_pose(data: &str) -> Result<WinlatorPoseData, UdpError> {
    let parts: Vec<&str> = data.split_whitespace().collect();

    if parts.len() < 30 {
        return Err(UdpError::ParseError(format!(
            "Invalid pose data: expected at least 30 fields, got {}",
            parts.len()
        )));
    }

    let buttons_str = parts[30];
    let flags_str = if parts.len() > 31 { parts[31] } else { "" };

    Ok(WinlatorPoseData {
        left_hand_quat: Quat::from_array([
            parts[1].parse().map_err(|e| UdpError::ParseError(format!("Left hand quat X: {}", e)))?,
            parts[2].parse().map_err(|e| UdpError::ParseError(format!("Left hand quat Y: {}", e)))?,
            parts[3].parse().map_err(|e| UdpError::ParseError(format!("Left hand quat Z: {}", e)))?,
            parts[4].parse().map_err(|e| UdpError::ParseError(format!("Left hand quat W: {}", e)))?,
        ]),
        left_hand_thumb: Vec2::new(
            parts[5].parse().map_err(|e| UdpError::ParseError(format!("Left thumb X: {}", e)))?,
            parts[6].parse().map_err(|e| UdpError::ParseError(format!("Left thumb Y: {}", e)))?,
        ),
        left_hand_pos: Vec3::new(
            parts[7].parse().map_err(|e| UdpError::ParseError(format!("Left hand X: {}", e)))?,
            parts[8].parse().map_err(|e| UdpError::ParseError(format!("Left hand Y: {}", e)))?,
            parts[9].parse().map_err(|e| UdpError::ParseError(format!("Left hand Z: {}", e)))?,
        ),
        right_hand_quat: Quat::from_array([
            parts[10].parse().map_err(|e| UdpError::ParseError(format!("Right hand quat X: {}", e)))?,
            parts[11].parse().map_err(|e| UdpError::ParseError(format!("Right hand quat Y: {}", e)))?,
            parts[12].parse().map_err(|e| UdpError::ParseError(format!("Right hand quat Z: {}", e)))?,
            parts[13].parse().map_err(|e| UdpError::ParseError(format!("Right hand quat W: {}", e)))?,
        ]),
        right_hand_thumb: Vec2::new(
            parts[14].parse().map_err(|e| UdpError::ParseError(format!("Right thumb X: {}", e)))?,
            parts[15].parse().map_err(|e| UdpError::ParseError(format!("Right thumb Y: {}", e)))?,
        ),
        right_hand_pos: Vec3::new(
            parts[16].parse().map_err(|e| UdpError::ParseError(format!("Right hand X: {}", e)))?,
            parts[17].parse().map_err(|e| UdpError::ParseError(format!("Right hand Y: {}", e)))?,
            parts[18].parse().map_err(|e| UdpError::ParseError(format!("Right hand Z: {}", e)))?,
        ),
        hmd_quat: Quat::from_array([
            parts[19].parse().map_err(|e| UdpError::ParseError(format!("HMD quat X: {}", e)))?,
            parts[20].parse().map_err(|e| UdpError::ParseError(format!("HMD quat Y: {}", e)))?,
            parts[21].parse().map_err(|e| UdpError::ParseError(format!("HMD quat Z: {}", e)))?,
            parts[22].parse().map_err(|e| UdpError::ParseError(format!("HMD quat W: {}", e)))?,
        ]),
        hmd_pos: Vec3::new(
            parts[23].parse().map_err(|e| UdpError::ParseError(format!("HMD X: {}", e)))?,
            parts[24].parse().map_err(|e| UdpError::ParseError(format!("HMD Y: {}", e)))?,
            parts[25].parse().map_err(|e| UdpError::ParseError(format!("HMD Z: {}", e)))?,
        ),
        ipd: parts[26].parse().map_err(|e| UdpError::ParseError(format!("IPD: {}", e)))?,
        fov_h: parts[27].parse().map_err(|e| UdpError::ParseError(format!("FOV H: {}", e)))?,
        fov_v: parts[28].parse().map_err(|e| UdpError::ParseError(format!("FOV V: {}", e)))?,
        frame_id: parts[29].parse().map_err(|e| UdpError::ParseError(format!("Frame ID: {}", e)))?,
        buttons: parse_winlator_buttons(buttons_str)?,
        immersive_mode: flags_str.starts_with('T'),
        sbs_mode: if flags_str.len() > 1 { flags_str.chars().nth(1) == Some('T') } else { false },
    })
}

fn parse_winlator_buttons(buttons_str: &str) -> Result<[bool; 19], UdpError> {
    let mut buttons = [false; 19];
    for (i, ch) in buttons_str.chars().enumerate() {
        if i >= 19 {
            break;
        }
        buttons[i] = ch == 'T';
    }
    Ok(buttons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_winlator_pose() {
        let sample = "client0 0.213 0.287 -0.933 0.035 0.0 0.0 -0.008 -0.229 -0.173 0.095 -0.296 0.947 -0.077 0.0 0.0 0.154 -0.240 -0.140 0.146 -0.072 0.048 0.985 0.037 0.006 -0.017 0.0678 99.00 103.40 224 TFFFFFFFFFTTTFFFFFT TT";

        let pose = parse_winlator_pose(sample);
        assert!(pose.is_ok(), "Should parse valid pose data");

        let pose = pose.unwrap();
        assert_eq!(pose.frame_id, 224);
        assert_eq!(pose.fov_h, 99.00);
        assert_eq!(pose.fov_v, 103.40);
    }

    #[test]
    fn test_parse_winlator_buttons() {
        let buttons_str = "TFFFFFFFFFTTTFFFFFT";
        let buttons = parse_winlator_buttons(buttons_str).unwrap();

        assert!(buttons[0]);  // Left Grip
        assert!(!buttons[1]); // Left Menu
        assert!(buttons[10]); // Right Button A
        assert!(buttons[18]); // Right Trigger
    }

    #[test]
    fn startup_packet_matches_protocol_example() {
        let data = WinlatorHapticData {
            left_vibration: 0.0,
            right_vibration: 0.0,
            vr_flag: 1,
            sbs_flag: false,
            target_fov_w: 104.5,
            target_fov_h: 104.5,
        };
        assert_eq!(format_haptic_message(&data), "0 0 1 0 104.5 104.5");
    }

    #[test]
    fn haptic_message_formats_sbs_and_vibration() {
        let data = WinlatorHapticData {
            left_vibration: 0.5,
            right_vibration: 0.25,
            vr_flag: 2,
            sbs_flag: true,
            target_fov_w: 90.0,
            target_fov_h: 95.5,
        };
        assert_eq!(format_haptic_message(&data), "0.5 0.25 2 1 90 95.5");
    }
}