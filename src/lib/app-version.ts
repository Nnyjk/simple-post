/**
 * 打包时内置的版本号。设置里「当前版本」优先读它，故显示版本不依赖网络，也不依赖
 * 后端是否已经报过一次版本。
 */
import pkg from '../../package.json';

export const APP_VERSION: string = typeof pkg.version === 'string' ? pkg.version : '';
