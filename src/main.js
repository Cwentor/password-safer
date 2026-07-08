// ============================================
// 应用入口
// 平台检测 + 动态加载 desktop/index.js 或 mobile/index.js
// ============================================

import { isMobile } from './shared/lib/platform.js';

async function bootstrap() {
  const app = document.getElementById('app');
  if (isMobile()) {
    const { mount } = await import('./mobile/index.js');
    mount(app);
  } else {
    const { mount } = await import('./desktop/index.js');
    mount(app);
  }
}

bootstrap();
