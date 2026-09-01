package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;

import java.io.ByteArrayInputStream;
import java.io.ObjectInputStream;
import java.io.ObjectOutputStream;
import java.io.ByteArrayOutputStream;





























@RestController
public class ProfileController {

    private final AuditLogVault vault = new AuditLogVault();

    


    @PostMapping("/benchmark/multibx/profile")
    public String fetchProfile(@RequestBody byte[] payload) throws Exception {
        // 中间节点：不可信流经无白名单反序列化成 ProfileData
        ProfileData data = deserialize(payload); // 反序列化行（中间节点，见本文件下方方法）

        // 第一环交互：把反序列化出的 userId 交给审计日志（信息泄露联动）
        String leakedUserId = data.getUserId();
        vault.logAccess(leakedUserId);

        /*ANCHOR_1*/
        return readProfile(leakedUserId); // 越权读他人数据：无归属校验的敏感读
    }

    


    static ProfileData deserialize(byte[] payload) throws Exception {
        ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(payload));
        Object obj = ois.readObject(); // 中间节点：反序列化到达（无白名单）
        ois.close();
        return (ProfileData) obj;
    }

    


    static String readProfile(String userId) {
        // 语义等价：profileRepository.findByUserId(userId) —— 越权读取他人数据
        System.out.println("[profile-read] userId=" + userId);
        return "profile-data:" + userId;
    }

    
    static byte[] serialize(ProfileData p) throws Exception {
        ByteArrayOutputStream bos = new ByteArrayOutputStream();
        ObjectOutputStream oos = new ObjectOutputStream(bos);
        oos.writeObject(p);
        oos.close();
        return bos.toByteArray();
    }
}
