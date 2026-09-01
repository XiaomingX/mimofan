/*
 * JSEF Benchmark 真假混淆样本 — IDOR 安全版（D5，CWE-639，L4）
 * BX 版：在返回对象前显式校验"当前用户是否为对象拥有者"，否则抛 403。
 * 测试点：强 SAST/LLM 应识别此处已做归属校验而不报；弱工具易误报（测 FP）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
import java.util.Optional;

public class IdorBy {

    static final class User { final String id; User(String id){ this.id = id; } }
    static final class Doc { final String id; final String ownerId; final String content;
        Doc(String id, String ownerId, String content){ this.id=id; this.ownerId=ownerId; this.content=content; } }

    interface DocRepository { Optional<Doc> findById(String id); }

    


    static Doc getDocument(DocRepository repo, String id, User currentUser) {
        Doc doc = repo.findById(id).orElseThrow();
        // 归属校验：阻断越权
        /*ANCHOR_1*/
        if (!doc.ownerId.equals(currentUser.id)) {
            throw new SecurityException("403 forbidden: not owner");
        }
        return doc;
    }
}
