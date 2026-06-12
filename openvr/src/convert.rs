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
           ..Default::default()
        };
    }

    TrackedDevicePose_t {
        mDeviceToAbsoluteTracking: HmdMatrix34_t::from((pos, rot)),
        vVelocity: HmdVector3_t { v: [0.0, 0.0, 0.0] },
        vAngularVelocity: HmdVector3_t { v: [0.0, 0.0, 0.0] },
        eTrackingResult: ETrackingResult::Running_OK,
        bPoseIsValid: true,
        bDeviceIsConnected: true,
       ..Default::default()
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
    fn from((pos, rot): (Vec3, Quat)) -> Self {
        let rot = Mat3::from_quat(rot).transpose();
        Self {
            m: [
                [rot.x_axis.x, rot.y_axis.x, rot.z_axis.x, pos.x],
                [rot.x_axis.y, rot.y_axis.y, rot.z_axis.y, pos.y],
                [rot.x_axis.z, rot.y_axis.z, rot.z_axis.z, pos.z],
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