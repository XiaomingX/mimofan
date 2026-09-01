/*
 * JSEF Benchmark 样本 — XStream 不安全反序列化 (CWE-502, L3)
 * 使用默认 XStream 解析不可信 XML，可触发任意对象构造（gadget）。
 * 安全底线：仅 localhost 演示语义，不提供 gadget payload。
 */
package blinded;

import com.thoughtworks.xstream.XStream;

public class XStreamBx {

    static Object fromXml(String xml) {
        XStream xs = new XStream(); // 默认无类型白名单
        /*ANCHOR_1*/
        return xs.fromXML(xml); // 反序列化任意类型
    }
}
