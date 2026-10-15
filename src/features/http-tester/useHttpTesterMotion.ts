/**
 * 请求栏与复制按钮的 GSAP 反馈动画。
 *
 * 两段动画与请求本身无关，只读 `requestPending` / `curlFlash` 两个状态，故连同
 * 它们各自的 ref 一起搬到这里，让 `HttpTester` 只留状态编排与 JSX。
 */
import { useRef } from 'react';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';

/**
 * 返回三个需要挂到 DOM 上的 ref：Send 按钮（微弹）、包住 Play 图标的 span（旋转）、
 * 复制 cURL 按钮（弹跳）。
 *
 * 调用点必须在任何提前返回之上——它们是 hook。
 */
export function useHttpTesterMotion(requestPending: boolean, curlFlash: boolean) {
  // C2：Send 按钮形变。`playIconRef` 是包住 Play 图标的 `inline-flex` span，就地
  // 旋转而不动按钮文字宽度；`sendBtnRef` 是整个 Button，用于请求返回后的微弹。
  const sendBtnRef = useRef<HTMLButtonElement>(null);
  const playIconRef = useRef<HTMLSpanElement>(null);
  // 上一次运行时是否真的有请求在途。`useGSAP` 挂载时也会跑一次，而挂载时
  // `requestPending` 为 false —— 不加这个门，每次打开接口标签都会弹一下，毫无意义。
  // 微弹要表达「响应到了」，故须以确实有请求可到达为前提。
  const wasPendingRef = useRef(false);

  useGSAP(() => {
    if (!playIconRef.current || !sendBtnRef.current) return;

    if (requestPending) {
      wasPendingRef.current = true;
      // 重新起转前先杀掉残留旋转（例如上次取消留下的）。gsap.fromTo 配 rotation +=360
      // + repeat:-1 得到连续匀速旋转。
      //
      // 受 reduced motion 限制，因为这个动画不会停：无尽循环是最强的前庭触发器，也是
      // 此处唯一在用户纯等待时运行的动画。不转不损失信息 —— 请求在途时按钮已
      // `disabled` 且标签是 发送中…，状态从不依赖旋转。
      gsap.killTweensOf(playIconRef.current);
      return motionSafe(() => {
        gsap.fromTo(
          playIconRef.current,
          { rotation: 0 },
          { rotation: '+=360', duration: 1, repeat: -1, ease: 'none' },
        );
      });
    }

    // 结束路径：杀掉旋转并把图标复位竖直，使下次渲染从 rotation 0 起。
    const requestArrived = wasPendingRef.current;
    wasPendingRef.current = false;
    gsap.killTweensOf(playIconRef.current);
    gsap.set(playIconRef.current, { rotation: 0 });
    if (!requestArrived) return;

    // 微弹按钮，确认响应已到。
    return motionSafe(() => {
      gsap.killTweensOf(sendBtnRef.current);
      gsap.fromTo(
        sendBtnRef.current,
        { scale: 1 },
        {
          keyframes: [
            { scale: 1.05, duration: 0.125, ease: 'power2.out' },
            { scale: 1, duration: 0.125, ease: 'power2.in' },
          ],
        },
      );
    });
  }, [requestPending]);

  // C3：复制 cURL 的反馈弹跳。仅在 curlFlash 由 false → true 时触发；false 那一跳
  // 刻意为空操作，免得已换成「已复制」的标签退出时再多闪一下。
  const copyCurlBtnRef = useRef<HTMLButtonElement>(null);
  useGSAP(() => {
    if (!curlFlash || !copyCurlBtnRef.current) return;
    return motionSafe(() => {
      gsap.killTweensOf(copyCurlBtnRef.current);
      gsap.fromTo(
        copyCurlBtnRef.current,
        { scale: 1 },
        {
          keyframes: [
            { scale: 1.06, duration: 0.1, ease: 'power2.out' },
            { scale: 1, duration: 0.1, ease: 'power2.in' },
          ],
        },
      );
    });
  }, [curlFlash]);

  return { sendBtnRef, playIconRef, copyCurlBtnRef };
}
