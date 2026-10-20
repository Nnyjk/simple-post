//! 宿主 API 的全部实现：一段固定的 JavaScript 前导。
//!
//! **Rust 侧不向引擎注册任何回调函数。** 宿主能力 = 这段前导里显式给出的那几样，
//! 加上 QuickJS 标准库（它没有任何 I/O）。于是「脚本够不到文件系统 / 网络 / 任意进程」
//! 不靠逐个禁用，而靠**根本没有那条路**。
//!
//! 宿主与脚本之间的唯一通路是 JSON：宿主调 `__SP_INIT__` 注入输入，跑完用户脚本后调
//! `__SP_DRAIN__` 取走改动与诊断。前导里的全局（`__SP_*`）是普通对象，脚本可以看见，
//! 但它们只搬运数据，不授予能力。

/// 前导源码。改动它等于改动公开的脚本 API。
pub const PRELUDE: &str = r##"
(function () {
  'use strict';

  var OUT = { request: null, envWrites: [], logs: [], assertions: [] };
  globalThis.__SP_OUT__ = OUT;

  var LIMITS = { maxLogs: 500, maxAssertions: 200 };

  var envState = { id: null, values: {} };
  var varState = { values: {} };
  var reqState = { method: '', url: '', headers: {}, body: null };
  var respState = null;

  function show(v) {
    if (typeof v === 'string') return v;
    if (v === undefined) return 'undefined';
    if (v === null) return 'null';
    try {
      var s = JSON.stringify(v);
      return s === undefined ? String(v) : s;
    } catch (e) {
      return String(v);
    }
  }

  function deepEqual(a, b) {
    if (a === b) return true;
    if (typeof a !== typeof b) return false;
    if (a === null || b === null) return false;
    if (typeof a !== 'object') {
      return typeof a === 'number' && typeof b === 'number' && isNaN(a) && isNaN(b);
    }
    if (Array.isArray(a) !== Array.isArray(b)) return false;
    var ka = Object.keys(a);
    var kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    for (var i = 0; i < ka.length; i++) {
      var k = ka[i];
      if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
      if (!deepEqual(a[k], b[k])) return false;
    }
    return true;
  }

  function typeName(v) {
    if (v === null) return 'null';
    if (Array.isArray(v)) return 'array';
    return typeof v;
  }

  // ---- 断言（chai 的一个子集）----------------------------------------

  function Expectation(actual) {
    this.actual = actual;
    this.negated = false;
  }

  var CHAIN = ['to', 'be', 'been', 'is', 'that', 'and', 'has', 'have', 'with',
               'at', 'of', 'same', 'but', 'does', 'still'];
  for (var ci = 0; ci < CHAIN.length; ci++) {
    (function (key) {
      Object.defineProperty(Expectation.prototype, key, {
        get: function () { return this; },
        configurable: true
      });
    })(CHAIN[ci]);
  }

  Object.defineProperty(Expectation.prototype, 'not', {
    get: function () {
      var next = Object.create(Expectation.prototype);
      next.actual = this.actual;
      next.negated = !this.negated;
      return next;
    },
    configurable: true
  });

  Expectation.prototype._assert = function (cond, message) {
    if (this.negated) {
      if (cond) throw new Error('expected not ' + message);
      return;
    }
    if (!cond) throw new Error(message);
  };

  Expectation.prototype.equal = function (expected) {
    this._assert(this.actual === expected,
      'expected ' + show(expected) + ' but got ' + show(this.actual));
  };

  Expectation.prototype.eql = function (expected) {
    this._assert(deepEqual(this.actual, expected),
      'expected ' + show(expected) + ' to deeply equal ' + show(this.actual));
  };

  Expectation.prototype.a = function (type) {
    this._assert(typeName(this.actual) === type,
      'expected ' + show(this.actual) + ' to be a ' + type + ' but it is a ' + typeName(this.actual));
  };
  Expectation.prototype.an = Expectation.prototype.a;

  Expectation.prototype.ok = function () {
    this._assert(Boolean(this.actual), 'expected ' + show(this.actual) + ' to be truthy');
  };

  Expectation.prototype.true = function () {
    this._assert(this.actual === true, 'expected ' + show(this.actual) + ' to be true');
  };

  Expectation.prototype.false = function () {
    this._assert(this.actual === false, 'expected ' + show(this.actual) + ' to be false');
  };

  Expectation.prototype.include = function (needle) {
    var actual = this.actual;
    var ok = false;
    if (typeof actual === 'string') ok = actual.indexOf(needle) !== -1;
    else if (Array.isArray(actual)) ok = actual.indexOf(needle) !== -1;
    this._assert(ok, 'expected ' + show(actual) + ' to include ' + show(needle));
  };

  Expectation.prototype.length = function (n) {
    var actual = this.actual;
    var len = (actual === null || actual === undefined) ? undefined : actual.length;
    this._assert(len === n, 'expected length ' + n + ' but got ' + show(len));
  };

  // `expect(code).to.have.status(200)` 与 `pm.response.to.have.status(200)` 两种写法
  // 都得能用，故这里既接受裸状态码，也接受带 `code` 字段的响应对象。
  Expectation.prototype.status = function (code) {
    var actual = this.actual;
    if (actual && typeof actual === 'object' && !Array.isArray(actual) && 'code' in actual) {
      actual = actual.code;
    }
    this._assert(actual === code, 'expected status ' + code + ' but got ' + show(actual));
  };

  function expect(actual) { return new Expectation(actual); }

  // ---- 请求视图 -------------------------------------------------------

  function normalizeHeaders(v) {
    var out = {};
    if (v === null || v === undefined) return out;
    if (Array.isArray(v)) {
      for (var i = 0; i < v.length; i++) {
        var h = v[i];
        if (h && h.key) {
          out[String(h.key)] = (h.value === null || h.value === undefined) ? '' : String(h.value);
        }
      }
      return out;
    }
    if (typeof v === 'object') {
      var keys = Object.keys(v);
      for (var j = 0; j < keys.length; j++) {
        var val = v[keys[j]];
        out[keys[j]] = (val === null || val === undefined) ? '' : String(val);
      }
    }
    return out;
  }

  function findHeaderKey(headers, name) {
    var target = String(name).toLowerCase();
    var keys = Object.keys(headers);
    for (var i = 0; i < keys.length; i++) {
      if (keys[i].toLowerCase() === target) return keys[i];
    }
    return null;
  }

  var requestView = {
    get method() { return reqState.method; },
    set method(v) { reqState.method = (v === null || v === undefined) ? '' : String(v); },
    get url() { return reqState.url; },
    set url(v) { reqState.url = (v === null || v === undefined) ? '' : String(v); },
    get headers() { return reqState.headers; },
    set headers(v) { reqState.headers = normalizeHeaders(v); },
    get body() { return reqState.body; },
    set body(v) { reqState.body = (v === null || v === undefined) ? null : String(v); },
    getHeader: function (name) {
      var key = findHeaderKey(reqState.headers, name);
      return key === null ? undefined : reqState.headers[key];
    },
    setHeader: function (name, value) {
      var key = findHeaderKey(reqState.headers, name);
      reqState.headers[key === null ? String(name) : key] = String(value);
    },
    addHeader: function (name, value) {
      reqState.headers[String(name)] = String(value);
    },
    removeHeader: function (name) {
      var key = findHeaderKey(reqState.headers, name);
      if (key !== null) delete reqState.headers[key];
    },
    hasHeader: function (name) {
      return findHeaderKey(reqState.headers, name) !== null;
    }
  };

  // ---- 变量视图 -------------------------------------------------------

  function clearMap(map) {
    var keys = Object.keys(map);
    for (var i = 0; i < keys.length; i++) delete map[keys[i]];
  }

  function makeVarView(store, persist) {    return {
      get: function (name) {
        return Object.prototype.hasOwnProperty.call(store, name) ? store[name] : undefined;
      },
      set: function (name, value) {
        var key = String(name);
        var v = (value === null || value === undefined) ? '' : String(value);
        store[key] = v;
        if (persist) OUT.envWrites.push({ key: key, value: v });
      },
      unset: function (name) {
        var key = String(name);
        delete store[key];
        if (persist) OUT.envWrites.push({ key: key, value: null });
      },
      has: function (name) {
        return Object.prototype.hasOwnProperty.call(store, name);
      },
      toObject: function () {
        var out = {};
        var keys = Object.keys(store);
        for (var i = 0; i < keys.length; i++) out[keys[i]] = store[keys[i]];
        return out;
      }
    };
  }

  var envView = makeVarView(envState.values, true);
  var varView = makeVarView(varState.values, false);

  // ---- 响应视图 -------------------------------------------------------

  var responseView = {
    get code() { return respState ? respState.code : undefined; },
    get status() { return respState ? respState.status : undefined; },
    get headers() { return respState ? respState.headers : {}; },
    get responseTime() { return respState ? respState.responseTime : undefined; },
    text: function () { return respState ? respState.body : ''; },
    json: function () {
      if (!respState) return null;
      try {
        return JSON.parse(respState.body);
      } catch (e) {
        throw new Error('response body is not valid JSON');
      }
    },
    get to() { return new Expectation(respState); }
  };

  // ---- pm -------------------------------------------------------------

  var pm = {
    request: requestView,
    environment: envView,
    variables: varView,
    response: responseView,
    expect: expect,
    test: function (name, fn) {
      var passed = true;
      var err = null;
      try {
        if (typeof fn !== 'function') throw new Error('pm.test expects a function');
        fn();
      } catch (e) {
        passed = false;
        err = (e && e.message) ? String(e.message) : String(e);
      }
      if (OUT.assertions.length < LIMITS.maxAssertions) {
        OUT.assertions.push({ name: String(name), passed: passed, error: err });
      }
    }
  };
  globalThis.pm = pm;

  // ---- console --------------------------------------------------------
  //
  // 日志进内存数组，**不写宿主 stdout**：MCP 守护进程的 stdout 只允许 JSON-RPC 帧。

  function makeConsole() {
    var levels = ['log', 'info', 'warn', 'error'];
    var c = {};
    for (var i = 0; i < levels.length; i++) {
      (function (level) {
        c[level] = function () {
          if (OUT.logs.length >= LIMITS.maxLogs) return;
          var parts = [];
          for (var j = 0; j < arguments.length; j++) parts.push(show(arguments[j]));
          OUT.logs.push({ level: level, message: parts.join(' ') });
        };
      })(levels[i]);
    }
    return c;
  }
  globalThis.console = makeConsole();

  // ---- 宿主接口 -------------------------------------------------------

  globalThis.__SP_INIT__ = function (input, limits) {
    if (limits) {
      LIMITS.maxLogs = limits.maxLogs;
      LIMITS.maxAssertions = limits.maxAssertions;
    }
    envState.id = input.environment ? (input.environment.id || null) : null;
    // 清空而不是换新对象：视图持有的是这两个对象的引用，换掉它们视图就看不到了。
    clearMap(envState.values);
    if (input.environment && input.environment.variables) {
      var ek = Object.keys(input.environment.variables);
      for (var i = 0; i < ek.length; i++) {
        envState.values[ek[i]] = String(input.environment.variables[ek[i]]);
      }
    }
    clearMap(varState.values);
    if (input.variables) {
      var vk = Object.keys(input.variables);
      for (var j = 0; j < vk.length; j++) varState.values[vk[j]] = String(input.variables[vk[j]]);
    }
    reqState.method = input.request ? String(input.request.method || '') : '';
    reqState.url = input.request ? String(input.request.url || '') : '';
    reqState.headers = normalizeHeaders(input.request ? input.request.headers : null);
    reqState.body = (input.request && input.request.body !== null && input.request.body !== undefined)
      ? String(input.request.body) : null;
    respState = input.response ? input.response : null;
  };

  // 失败的脚本要回滚：宿主把它改过的请求换回来。
  globalThis.__SP_SET_REQUEST__ = function (req) {
    if (!req) return false;
    reqState.method = String(req.method || '');
    reqState.url = String(req.url || '');
    reqState.headers = normalizeHeaders(req.headers);
    reqState.body = (req.body === null || req.body === undefined) ? null : String(req.body);
    return true;
  };

  // 取走本段脚本的产出，并把累积区清空：下一段脚本的产出因此只含它自己的。
  globalThis.__SP_DRAIN__ = function () {
    var payload = {
      request: reqState ? {
        method: reqState.method,
        url: reqState.url,
        headers: reqState.headers,
        body: reqState.body
      } : null,
      envWrites: OUT.envWrites,
      logs: OUT.logs,
      assertions: OUT.assertions
    };
    OUT.envWrites = [];
    OUT.logs = [];
    OUT.assertions = [];
    return payload;
  };
})();
"##;
