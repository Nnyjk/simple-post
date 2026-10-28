/**
 * 偏好键与主题解析的行为断言。
 *
 * 这个模块是 `index.html` 的首帧脚本与 store 共同依赖的那一份，故它的取值必须对
 * 「存下来的任意内容」都给出确定的答案：非法主题回落成缺省的 `dark`，坏掉的 JSON
 * 当作没有偏好。
 */
import { describe, it, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import themeBoot from '../index';

interface FakeBrowser {
  storage: Map<string, string>;
  systemDark: boolean;
  /** `<html>` 上有没有 `dark` 这个 class。 */
  darkClass: boolean;
  restore: () => void;
}

/**
 * 装一套假的浏览器环境。
 *
 * 本模块只用到三样东西：`window.localStorage`、`window.matchMedia` 与
 * `document.documentElement.classList`，故假环境也只给这三样。
 */
function installFakeBrowser(stored: Record<string, string> = {}, systemDark = false): FakeBrowser {
  const g = globalThis as any;
  const before = {
    window: g.window,
    document: g.document,
  };
  const storage = new Map<string, string>(Object.entries(stored));
  const fake = {
    storage,
    systemDark,
    darkClass: false,
    restore() {
      if (before.window === undefined) delete g.window;
      else g.window = before.window;
      if (before.document === undefined) delete g.document;
      else g.document = before.document;
    },
  };

  g.window = {
    localStorage: {
      getItem: (k: string) => (storage.has(k) ? storage.get(k)! : null),
      setItem: (k: string, v: string) => storage.set(k, String(v)),
      removeItem: (k: string) => storage.delete(k),
    },
    matchMedia: (query: string) => ({
      matches: query.includes('dark') ? systemDark : false,
      media: query,
    }),
  };
  g.document = {
    documentElement: {
      classList: {
        toggle: (name: string, on: boolean) => {
          if (name === 'dark') fake.darkClass = on;
        },
      },
    },
  };
  return fake;
}

afterEach(() => {
  const g = globalThis as any;
  delete g.window;
  delete g.document;
});

describe('主题取值的校验', () => {
  it('三个合法取值原样返回', () => {
    assert.equal(themeBoot.resolveTheme('dark'), 'dark');
    assert.equal(themeBoot.resolveTheme('light'), 'light');
    assert.equal(themeBoot.resolveTheme('system'), 'system');
  });

  it('其余取值一律回落到缺省的 dark', () => {
    // 旧版本可能留下别的名字，用户也可能手改坏 localStorage。
    assert.equal(themeBoot.resolveTheme('blue'), 'dark');
    assert.equal(themeBoot.resolveTheme(''), 'dark');
    assert.equal(themeBoot.resolveTheme(null), 'dark');
    assert.equal(themeBoot.resolveTheme(undefined), 'dark');
    assert.equal(themeBoot.resolveTheme(42), 'dark');
    assert.equal(themeBoot.resolveTheme({ theme: 'light' }), 'dark');
    assert.equal(themeBoot.resolveTheme(['light']), 'dark');
  });
});

describe('主题要不要画成深色', () => {
  it('dark 是深色，light 是浅色', () => {
    assert.equal(themeBoot.prefersDark('dark'), true);
    assert.equal(themeBoot.prefersDark('light'), false);
  });

  it('system 问操作系统', () => {
    const browser = installFakeBrowser({}, true);
    try {
      assert.equal(themeBoot.prefersDark('system'), true);
    } finally {
      browser.restore();
    }

    const light = installFakeBrowser({}, false);
    try {
      assert.equal(themeBoot.prefersDark('system'), false);
    } finally {
      light.restore();
    }
  });
});

describe('偏好原文的解析', () => {
  it('坏掉的 JSON 当作没有偏好', () => {
    assert.deepEqual(themeBoot.parsePrefs('{not json'), {});
    assert.deepEqual(themeBoot.parsePrefs(''), {});
    assert.deepEqual(themeBoot.parsePrefs(null), {});
  });

  it('null 与数组不是偏好的形状', () => {
    assert.deepEqual(themeBoot.parsePrefs('null'), {});
    assert.deepEqual(themeBoot.parsePrefs('[]'), {});
    assert.deepEqual(themeBoot.parsePrefs('42'), {});
  });

  it('合法的那一份原样读出来', () => {
    assert.deepEqual(themeBoot.parsePrefs('{"theme":"light","lastProjectId":"p1"}'), {
      theme: 'light',
      lastProjectId: 'p1',
    });
  });
});

describe('读偏好', () => {
  it('读不出偏好时主题是缺省的 dark', () => {
    const browser = installFakeBrowser();
    try {
      assert.deepEqual(themeBoot.readPersistedPrefs(), {});
      assert.equal(themeBoot.readPersistedTheme(), 'dark');
    } finally {
      browser.restore();
    }
  });

  it('读出存下来的主题', () => {
    const browser = installFakeBrowser({ 'simple-post:prefs': '{"theme":"light"}' });
    try {
      assert.equal(themeBoot.readPersistedTheme(), 'light');
    } finally {
      browser.restore();
    }
  });

  it('存下来的主题不合法时回落到 dark', () => {
    const browser = installFakeBrowser({ 'simple-post:prefs': '{"theme":"blue"}' });
    try {
      assert.equal(themeBoot.readPersistedTheme(), 'dark');
    } finally {
      browser.restore();
    }
  });
});

describe('把主题写到 <html> 上', () => {
  it('存的是 light 就不加 dark 这个 class', () => {
    const browser = installFakeBrowser({ 'simple-post:prefs': '{"theme":"light"}' });
    try {
      themeBoot.applyInitialTheme();
      assert.equal(browser.darkClass, false);
    } finally {
      browser.restore();
    }
  });

  it('存的是 dark 就加上 dark 这个 class', () => {
    const browser = installFakeBrowser({ 'simple-post:prefs': '{"theme":"dark"}' });
    try {
      themeBoot.applyInitialTheme();
      assert.equal(browser.darkClass, true);
    } finally {
      browser.restore();
    }
  });

  it('存的是 system 就问操作系统', () => {
    const browser = installFakeBrowser({ 'simple-post:prefs': '{"theme":"system"}' }, true);
    try {
      themeBoot.applyInitialTheme();
      assert.equal(browser.darkClass, true);
    } finally {
      browser.restore();
    }
  });

  it('没有偏好就按缺省的 dark', () => {
    const browser = installFakeBrowser();
    try {
      themeBoot.applyInitialTheme();
      assert.equal(browser.darkClass, true);
    } finally {
      browser.restore();
    }
  });
});
