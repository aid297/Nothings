// 根 crate 外部视角：真正拦截 E0603（src/ 内的单元测试测不出外部可见性问题）
#![allow(clippy::should_implement_trait)] // 宏生成的固有 default() 会触发该 lint

use nothings::impl_singleton;
use nothings::singletons::app::Singleton;

#[derive(Default)]
struct ExternalCounter {
    value: u32,
}

impl ExternalCounter {
    // 宏要求 default_new() 是固有方法；名字固定，可见性在宏调用处可见即可
    fn default_new() -> Self {
        ExternalCounter { value: 0 }
    }

    fn increment(&mut self) -> u32 {
        self.value += 1;
        self.value
    }
}

impl_singleton!(ExternalCounter);

#[test]
fn singleton_macro_works_from_external_crate() {
    // instance() 返回 &'static Mutex<_>
    let handle: &'static std::sync::Mutex<ExternalCounter> = ExternalCounter::instance();

    // with() 来自 Singleton trait：修改同一个全局实例并透传闭包返回值
    let first = ExternalCounter::with(|c| c.increment());
    let second = ExternalCounter::with(|c| c.increment());
    assert_eq!(second, first + 1, "with() 必须作用于同一个全局实例");

    // default() 每次新建，与单例无关
    let fresh = ExternalCounter::default();
    assert_eq!(fresh.value, 0);
    assert_eq!(fresh.value, <ExternalCounter as Default>::default().value);

    // 直接锁 handle 能观察到刚才的修改
    let guard = handle.lock().unwrap();
    assert_eq!(guard.value, second);
}
