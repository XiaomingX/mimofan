package blinded;

import org.w3c.dom.Document;
import org.w3c.dom.Element;
import org.w3c.dom.NodeList;

import javax.xml.crypto.dsig.Reference;
import javax.xml.crypto.dsig.XMLSignature;
import javax.xml.crypto.dsig.XMLSignatureFactory;
import javax.xml.crypto.dsig.dom.DOMValidateContext;
import javax.xml.parsers.DocumentBuilderFactory;
import java.io.ByteArrayInputStream;
import java.security.PublicKey;

/*
 * JSEF-Benchmark L5 — SAML 验签对象与鉴权读取对象一致（CWE-347 修复）
 *
 * 修复：鉴权只从 SignedInfo 实际引用的那份 <Assertion> 读取 role，
 * 校验被签名覆盖的节点与读取节点为同一引用，XSW 注入的外层副本被忽略。
 *
 * CWE-347 (Improper Verification of Cryptographic Signature)。
 */
public class SamlXswBy {

    static PublicKey loadPublicKey() {
        return null; // 演示桩：实际从 KeyStore/证书中读取
    }

    



    static Element referencedAssertion(Document doc, XMLSignature sig) throws Exception {
        for (Object o : sig.getSignedInfo().getReferences()) {
            Reference ref = (Reference) o;
            Element signed = (Element) ref.getDereferencedData(); // 解引用被签节点
            if (signed != null && "Assertion".equals(signed.getLocalName())) {
                return signed; // 只取签名覆盖的断言
            }
        }
        return null;
    }

    static boolean grantAccess(Element asserted) {
        return "admin".equals(asserted.getAttribute("role"));
    }

    public boolean authorize(String samlResponse) throws Exception {
        Document doc = DocumentBuilderFactory.newInstance()
                .newDocumentBuilder()
                .parse(new ByteArrayInputStream(samlResponse.getBytes("UTF-8")));

        NodeList sigList = doc.getElementsByTagNameNS(XMLSignature.XMLNS, "Signature");
        DOMValidateContext ctx = new DOMValidateContext(loadPublicKey(), sigList.item(0));
        XMLSignature sig = XMLSignatureFactory.getInstance("DOM").unmarshalXMLSignature(ctx);
        if (!sig.validate(ctx)) {
            return false;
        }

        Element asserted = referencedAssertion(doc, sig); // 鉴权读取与验签对象一致
        if (asserted == null) {
            return false;
        }
        /*ANCHOR_1*/
        return grantAccess(asserted); // 只基于签名覆盖的断言授权
    }

    public static void main(String[] args) throws Exception {
        new SamlXswBy().authorize("<samlp:Response>...</samlp:Response>");
    }
}
