use super::*;
use glam::{Affine3A, Mat3, Mat4, Quat, Vec3};

// WinlatorXR: no OpenXR — we build poses directly from UDP data

/// Create an OpenVR pose from a position + rotation.
/// `valid` comes from WinlatorXR tracking state.
pub fn space_relation_to_openvr_pose(pos: Vec3, rot: Quat, valid: bool) -> TrackedDevicePose_t {
    if!valid {
        return TrackedDevicePose_t {
            bPoseIsValid: false,
            bDeviceIsConnected: false,
            mDeviceToAbsoluteTracking: unsafe { std::mem::zeroed() },
            vVelocity: HmdVector3_t { v: [0.0, 0.0, 0.0] },
            vAngularVelocity: HmdVector3_t { v: [0.0, 0.0, 0.0] },
            eTrackingResult: ETrackingResult::Running_OutOfRange,
        };
    }

    TrackedDevicePose_t {
        mDeviceToAbsoluteTracking: HmdMatrix34_t::from((pos, rot)),
        vVelocity: HmdVector3_t { v: [0.0, 0.0, 0.0] },
        vAngularVelocity: HmdVector3_t { v: [0.0, 0.0, 0.0] },
        eTrackingResult: ETrackingResult::Running_OK,
        bPoseIsValid: true,
        bDeviceIsConnected: true,
    }
}

impl From<Mat4> for HmdMatrix44_t {
    fn from(value: Mat4) -> Self {
        Self { m: value.transpose().to_cols_array_2d() }
    }
}

impl From<Vec3> for HmdVector3_t {
    fn from(value: Vec3) -> Self { Self { v: value.to_array() } }
}

impl From<Vec3> for HmdVector4_t {
    fn from(value: Vec3) -> Self {
        let mut v = [0.0; 4];
        v[..3].copy_from_slice(&value.to_array());
        v[3] = 1.0;
        Self { v }
    }
}

impl From<Quat> for HmdQuaternionf_t {
    fn from(value: Quat) -> Self {
        Self { x: value.x, y: value.y, z: value.z, w: value.w }
    }
}

// Build OpenVR 3x4 matrix from (pos, rot)
impl From<(Vec3, Quat)> for HmdMatrix34_t {
    fn from((pos, q): (Vec3, Quat)) -> Self {
        // Row-major rotation, matching the layout produced by
        // `crate::winlatorxr::From<XrPosef>`.
        let x2 = q.x + q.x;
        let y2 = q.y + q.y;
        let z2 = q.z + q.z;
        let xx = q.x * x2;
        let xy = q.x * y2;
        let xz = q.x * z2;
        let yy = q.y * y2;
        let yz = q.y * z2;
        let zz = q.z * z2;
        let wx = q.w * x2;
        let wy = q.w * y2;
        let wz = q.w * z2;
        Self {
            m: [
                [1.0 - (yy + zz), xy - wz, xz + wy, pos.x],
                [xy + wz, 1.0 - (xx + zz), yz - wx, pos.y],
                [xz - wy, yz + wx, 1.0 - (xx + yy), pos.z],
            ],
        }
    }
}

impl From<Affine3A> for VRBoneTransform_t {
    fn from(value: Affine3A) -> Self {
        let (_, rot, pos) = value.to_scale_rotation_translation();
        Self { position: pos.into(), orientation: rot.into() }
    }
}