use alloy::sol;

sol! {
    interface IERC5564Announcer {
        event Announcement(
            uint256 indexed schemeId,
            address indexed stealthAddress,
            address indexed caller,
            bytes ephemeralPubKey,
            bytes metadata
        );
        function announce(
            uint256 schemeId,
            address stealthAddress,
            bytes memory ephemeralPubKey,
            bytes memory metadata
        ) external;
    }

    #[sol(rpc)]
    interface IERC6538Registry {
        function registerKeys(uint256 schemeId, bytes calldata stealthMetaAddress) external;
        function stealthMetaAddressOf(address registrant, uint256 schemeId)
            external
            view
            returns (bytes memory stealthMetaAddress);
    }
}
