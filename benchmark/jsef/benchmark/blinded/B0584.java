package blinded;

import java.io.ByteArrayInputStream;
import java.io.ObjectInputStream;
import java.util.Base64;





































public class ShiroRememberMeGadget {

    




    public static Object processRememberMe(String rememberMeCookie) {
        // ① 不可信源：rememberMe Cookie（Base64 字符串）
        byte[] decoded = Base64.getDecoder().decode(rememberMeCookie);                 // 52

        // ② 模拟 AES 解密：以硬编码密钥语义解开密文，得到原始对象字节（不可信）
        byte[] decrypted = aesDecrypt(decoded);                                        // 55

        // ③ 反序列化触发：解密后的不可信字节直接交给 ObjectInputStream.readObject
        /*ANCHOR_1*/
        try (ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(decrypted))) {
            return ois.readObject(); // ★ SINK：对任意 gadget 可达
        } catch (Exception e) {
            throw new RuntimeException("localhost-demo: deserialization entry reached", e);
        }
    }

    
    static byte[] aesDecrypt(byte[] ciphertext) {
        // 占位：真实场景此处用硬编码密钥解密；演示仅原样透传表示"已解密字节"
        return ciphertext;
    }

    public static void main(String[] args) {
        // 仅演示链路可达性，不连接真实网络/不使用真实 Cookie 与密钥
        // 传入一个 Base64 占位串，仅用于触发 readObject 入口演示
        processRememberMe("localhost-demo-placeholder");
    }
}
