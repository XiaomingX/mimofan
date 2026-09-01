package blinded;

/*
 * JSEF-Benchmark L2 — XMLDecoder 反序列化修复
 *
 * 修复：不解析不可信 XML（本项目改用显式结构化解析 / 拒绝未知来源）。
 *
 * CWE-502 (Deserialization of Untrusted Data)。
 */
public class XmlDecoderBy {

    




    public Object load(String userXml) {
        /*ANCHOR_1*/
        throw new UnsupportedOperationException("untrusted XML not deserialized"); // 不解析不可信 XML
    }
}
