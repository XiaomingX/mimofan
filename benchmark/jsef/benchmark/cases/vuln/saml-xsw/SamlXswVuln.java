package com.jsef.benchmark.vuln.samlxsw;

import org.w3c.dom.Document;
import org.w3c.dom.Element;
import org.w3c.dom.NodeList;

import javax.xml.crypto.dsig.XMLSignature;
import javax.xml.crypto.dsig.XMLSignatureFactory;
import javax.xml.crypto.dsig.dom.DOMValidateContext;
import javax.xml.parsers.DocumentBuilderFactory;
import java.io.ByteArrayInputStream;
import java.security.PublicKey;

/*
 * JSEF-Benchmark L5 — SAML XML 签名包裹（XML Signature Wrapping, XSW, CWE-347）
 *
 * 难度：L5（框架语义 / gadget 级组合）。"验签对象"与"鉴权读取对象"不一致：
 *   ① 验签：XMLSignature.validate() 仅对 SignedInfo 引用的那份 <Assertion> 验签，通过；
 *   ② 鉴权：document.getElementsByTagName("Assertion") 取"第一个" <Assertion> 读 role。
 * 攻击者可把已验签断言外层再包一层未签名 <Assertion>（XSW），
 * 应用读到的是注入副本，role 被伪造，鉴权被绕过。
 *
 * CWE-347 (Improper Verification of Cryptographic Signature)。
 * 安全底线：仅 localhost 演示语义，不提供真实攻击响应。
 * 修复要点（对照 SamlXswSafe.java）：鉴权必须只读 SignedInfo 实际引用的那份断言。
 */
public class SamlXswVuln {

    /** 语义等价：opensaml SignatureValidator 从可信证书加载公钥（仅 localhost 演示）。 */
    static PublicKey loadPublicKey() {
        return null; // 演示桩：实际从 KeyStore/证书中读取
    }

    /**
     * 验签位点：只对 SignedInfo 引用的那份 <Assertion> 做 XML 签名校验。
     * 攻击者包裹的未签名外层 <Assertion> 不在 SignedInfo 引用范围内，故验签仍通过。
     */
    static boolean verifySignature(Document doc) throws Exception {
        NodeList sigList = doc.getElementsByTagNameNS(XMLSignature.XMLNS, "Signature");
        if (sigList.getLength() == 0) {
            return false;
        }
        DOMValidateContext ctx = new DOMValidateContext(loadPublicKey(), sigList.item(0));
        XMLSignature sig = XMLSignatureFactory.getInstance("DOM").unmarshalXMLSignature(ctx);
        return sig.validate(ctx); // 验签通过：SignedInfo 引用的断言内容未被篡改
    }

    /** 鉴权位点：基于断言的 role 属性授权（仅 localhost 演示）。 */
    static boolean grantAccess(Element asserted) {
        return "admin".equals(asserted.getAttribute("role"));
    }

    /**
     * 危险路径：验签与鉴权读取分离。
     * 若断言被 XSW 包裹，item(0) 取到的是注入的未签名副本。
     *
     * @param samlResponse 攻击者可控 SAML 响应
     */
    public boolean authorize(String samlResponse) throws Exception {
        Document doc = DocumentBuilderFactory.newInstance()
                .newDocumentBuilder()
                .parse(new ByteArrayInputStream(samlResponse.getBytes("UTF-8")));

        boolean sigOk = verifySignature(doc); // ① 验签：对 SignedInfo 引用内容校验通过
        if (!sigOk) {
            return false;
        }

        NodeList assertions = doc.getElementsByTagName("Assertion"); // ② 读取：取第一个 Assertion
        Element asserted = (Element) assertions.item(0); // 注入的未签名副本可能在此被读到

        // [CHECKPOINT id=JSEF-SAML-001 cwe=347 level=L5 source=attacker-controlled SAML response sink=authorization based on unsigned injected assertion expect=VULN trace=benchmark/cases/vuln/saml-xsw/SamlXswVuln.java:45,benchmark/cases/vuln/saml-xsw/SamlXswVuln.java:70,benchmark/cases/vuln/saml-xsw/SamlXswVuln.java:73]
        return grantAccess(asserted); // [VULN] ③ 授权：基于可能被伪造的 role 放行
    }

    public static void main(String[] args) throws Exception {
        new SamlXswVuln().authorize("<samlp:Response>...</samlp:Response>");
    }
}
