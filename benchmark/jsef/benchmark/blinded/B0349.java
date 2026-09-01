package blinded;

import javax.xml.parsers.DocumentBuilder;
import javax.xml.parsers.DocumentBuilderFactory;
import java.io.ByteArrayInputStream;

/*
 * JSEF-Benchmark L3 — XXE 加固修复（CWE-611）
 *
 * 修复：在用于解析的同一 DocumentBuilderFactory 实例上设置 hardening。
 *
 * CWE-611 (Improper Restriction of XML External Entity Reference)。
 */
public class XxeWrongFactoryBy {

    static final String DISALLOW = "http://apache.org/xml/features/disallow-doctype-decl";

    




    public void parse(byte[] userXml) throws Exception {
        DocumentBuilderFactory dbf = DocumentBuilderFactory.newInstance();
        dbf.setFeature(DISALLOW, true); // 加固设于同一实例
        DocumentBuilder db = dbf.newDocumentBuilder();
        /*ANCHOR_1*/
        db.parse(new ByteArrayInputStream(userXml)); // 同一实例已禁用 DOCTYPE
    }

    public static void main(String[] args) throws Exception {
        new XxeWrongFactoryBy().parse("<x/>".getBytes());
    }
}
