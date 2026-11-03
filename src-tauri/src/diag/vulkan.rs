//! Vulkan 런타임(vulkan-1.dll)을 동적으로 불러와 GPU 목록을 조회한다.
//! 드라이버가 없거나 오래된 PC에서도 앱이 죽지 않도록 모든 실패를 오류 문자열로 돌려준다.

use ash::vk;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VulkanDevice {
    pub name: String,
    pub device_type: &'static str,
    pub api_version: String,
    pub driver_version: u32,
    pub vendor_id: u32,
    /// 장치 전용 메모리 힙 합계(바이트). 내장 GPU는 시스템 RAM 일부를 공유하므로 참고용.
    pub device_local_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VulkanInfo {
    pub available: bool,
    pub devices: Vec<VulkanDevice>,
    pub error: Option<String>,
}

pub fn probe() -> VulkanInfo {
    match probe_inner() {
        Ok(devices) => VulkanInfo {
            available: !devices.is_empty(),
            error: devices.is_empty().then(|| "Vulkan은 설치돼 있지만 사용할 수 있는 GPU가 없음".to_string()),
            devices,
        },
        Err(e) => VulkanInfo { available: false, devices: Vec::new(), error: Some(e) },
    }
}

fn probe_inner() -> Result<Vec<VulkanDevice>, String> {
    // SAFETY: 시스템 Vulkan 로더를 불러온다. 실패하면 오류만 돌려준다.
    let entry = unsafe { ash::Entry::load() }.map_err(|e| format!("Vulkan 런타임을 찾을 수 없음: {e}"))?;

    let app_name = c"tonghap-module-diag";
    let app_info = vk::ApplicationInfo::default()
        .application_name(app_name)
        .api_version(vk::make_api_version(0, 1, 1, 0));
    let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);

    // SAFETY: create_info와 app_info는 이 함수가 끝날 때까지 살아 있다.
    let instance = unsafe { entry.create_instance(&create_info, None) }
        .map_err(|e| format!("Vulkan 인스턴스를 만들 수 없음: {e:?}"))?;

    let result = (|| {
        // SAFETY: instance는 유효하며 아래에서 한 번만 파괴한다.
        let physical = unsafe { instance.enumerate_physical_devices() }
            .map_err(|e| format!("GPU 목록을 가져올 수 없음: {e:?}"))?;
        let mut devices = Vec::with_capacity(physical.len());
        for pd in physical {
            let props = unsafe { instance.get_physical_device_properties(pd) };
            let mem = unsafe { instance.get_physical_device_memory_properties(pd) };
            let name = props
                .device_name_as_c_str()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "(이름 없음)".to_string());
            let device_local_bytes = mem
                .memory_heaps_as_slice()
                .iter()
                .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
                .map(|h| h.size)
                .sum();
            devices.push(VulkanDevice {
                name,
                device_type: match props.device_type {
                    vk::PhysicalDeviceType::INTEGRATED_GPU => "내장 GPU",
                    vk::PhysicalDeviceType::DISCRETE_GPU => "외장 GPU",
                    vk::PhysicalDeviceType::VIRTUAL_GPU => "가상 GPU",
                    vk::PhysicalDeviceType::CPU => "CPU(소프트웨어)",
                    _ => "기타",
                },
                api_version: format!(
                    "{}.{}.{}",
                    vk::api_version_major(props.api_version),
                    vk::api_version_minor(props.api_version),
                    vk::api_version_patch(props.api_version)
                ),
                driver_version: props.driver_version,
                vendor_id: props.vendor_id,
                device_local_bytes,
            });
        }
        Ok(devices)
    })();

    // SAFETY: 위에서 만든 인스턴스를 정리한다.
    unsafe { instance.destroy_instance(None) };
    result
}

#[cfg(test)]
mod tests {
    /// Vulkan이 없는 환경에서도 패닉 없이 오류 정보를 돌려줘야 한다.
    #[test]
    fn probe_never_panics() {
        let info = super::probe();
        assert_eq!(info.available, !info.devices.is_empty());
        if !info.available {
            assert!(info.error.is_some());
        }
        eprintln!("vulkan available={} devices={} error={:?}", info.available, info.devices.len(), info.error);
    }
}
