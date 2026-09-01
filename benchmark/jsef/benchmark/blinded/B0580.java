package blinded;





















public class FastjsonCrossFile_B_Transport {

    
    private String typeName;

    



    public void setTypeName(String typeName) {
        this.typeName = typeName;   // B:24 污点写入字段
    }

    



    public String getTypeName() {
        return this.typeName;   // B:38 污点流出字段
    }
}
