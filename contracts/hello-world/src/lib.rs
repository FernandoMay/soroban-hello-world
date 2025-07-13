#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, 
    Env, String, Address, BytesN, Bytes, Map, Vec
};

// Savia Smart Contracts for Stellar Soroban
// Crowdfunding platform with NFT rewards and trust scoring

// ========== DATA STRUCTURES ==========

#[derive(Clone)]
#[contracttype]
pub struct Campaign {
    pub id: BytesN<32>,
    pub title: String,
    pub description: String,
    pub beneficiary: Address,
    pub goal_amount: u64,
    pub current_amount: u64,
    pub start_time: u64,
    pub end_time: u64,
    pub verified: bool,
    pub trust_score: u32,
    pub category: String,
    pub location: String,
    pub active: bool,
}

#[derive(Clone)]
#[contracttype]
pub struct Donation {
    pub id: BytesN<32>,
    pub campaign_id: BytesN<32>,
    pub donor: Address,
    pub amount: u64,
    pub timestamp: u64,
    pub nft_minted: bool,
    pub anonymous: bool,
}

#[derive(Clone)]
#[contracttype]
pub struct TrustScore {
    pub entity: Address,
    pub score: u32,
    pub verification_level: u32,
    pub donation_count: u32,
    pub total_donated: u64,
    pub campaigns_created: u32,
    pub last_updated: u64,
}

#[derive(Clone)]
#[contracttype]
pub struct NFTBadge {
    pub id: BytesN<32>,
    pub owner: Address,
    pub badge_type: String,
    pub campaign_id: Option<BytesN<32>>,
    pub minted_at: u64,
    pub metadata_uri: String,
}

#[derive(Clone)]
#[contracttype]
pub struct Disbursement {
    pub id: BytesN<32>,
    pub campaign_id: BytesN<32>,
    pub recipient: Address,
    pub amount: u64,
    pub milestone: String,
    pub status: DisbursementStatus,
    pub created_at: u64,
    pub executed_at: Option<u64>,
}

#[derive(Clone, PartialEq)]
#[contracttype]
pub enum DisbursementStatus {
    Pending,
    Approved,
    Executed,
    Rejected,
}

#[derive(Clone)]
#[contracttype]
pub struct PlatformStats {
    pub total_campaigns: u64,
    pub total_donations: u64,
    pub total_raised: u64,
    pub total_nfts: u64,
    pub active_campaigns: u64,
}

// ========== STORAGE KEYS ==========

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Campaign(BytesN<32>),
    Donation(BytesN<32>),
    TrustScore(Address),
    NFTBadge(BytesN<32>),
    Disbursement(BytesN<32>),
    PlatformFee,
    Admin,
    CampaignCounter,
    DonationCounter,
    NFTCounter,
    DisbursementCounter,
    Stats,
    CampaignsByBeneficiary(Address),
    DonationsByCampaign(BytesN<32>),
}

// ========== ERROR CODES ==========

#[derive(Clone, Copy, Debug, PartialEq)]
#[contracttype]
pub enum SaviaError {
    InvalidFee = 1,
    InvalidGoal = 2,
    InvalidDuration = 3,
    CampaignNotFound = 4,
    CampaignEnded = 5,
    InvalidAmount = 6,
    ScoreExists = 7,
    InsufficientFunds = 8,
    DisbursementNotFound = 9,
    NotApproved = 10,
    Unauthorized = 11,
    CampaignInactive = 12,
    InvalidInput = 13,
    AlreadyInitialized = 14,
}

impl From<SaviaError> for soroban_sdk::Error {
    fn from(e: SaviaError) -> Self {
        soroban_sdk::Error::from_contract_error(e as u32)
    }
}

// ========== MAIN CONTRACT ==========

#[contract]
pub struct SaviaContract;

#[contractimpl]
impl SaviaContract {
    
    /// Initialize the contract with platform fee and admin
    pub fn initialize(env: Env, admin: Address, platform_fee: u64) -> Result<(), SaviaError> {
        // Check if already initialized
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(SaviaError::AlreadyInitialized);
        }

        if platform_fee > 1000 { // Max 10% fee
            return Err(SaviaError::InvalidFee);
        }

        admin.require_auth();
        
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::PlatformFee, &platform_fee);
        env.storage().instance().set(&DataKey::CampaignCounter, &0u64);
        env.storage().instance().set(&DataKey::DonationCounter, &0u64);
        env.storage().instance().set(&DataKey::NFTCounter, &0u64);
        env.storage().instance().set(&DataKey::DisbursementCounter, &0u64);
        
        let initial_stats = PlatformStats {
            total_campaigns: 0,
            total_donations: 0,
            total_raised: 0,
            total_nfts: 0,
            active_campaigns: 0,
        };
        env.storage().instance().set(&DataKey::Stats, &initial_stats);
        
        Ok(())
    }

    /// Create a new campaign
    pub fn create_campaign(
        env: Env,
        beneficiary: Address,
        title: String,
        description: String,
        goal_amount: u64,
        duration_days: u64,
        category: String,
        location: String,
    ) -> Result<BytesN<32>, SaviaError> {
        beneficiary.require_auth();

        // Validate inputs
        if goal_amount == 0 {
            return Err(SaviaError::InvalidGoal);
        }
        
        if duration_days == 0 || duration_days > 365 {
            return Err(SaviaError::InvalidDuration);
        }

        if title.len() == 0 || description.len() == 0 {
            return Err(SaviaError::InvalidInput);
        }

        // Get and increment campaign counter
        let counter: u64 = env.storage().instance().get(&DataKey::CampaignCounter).unwrap_or(0);
        let new_counter = counter + 1;
        env.storage().instance().set(&DataKey::CampaignCounter, &new_counter);

        // Generate campaign ID
        let current_time = env.ledger().timestamp();
        let campaign_id = Self::generate_id(
            &env,
            &[
                beneficiary.to_string().as_bytes(),
                title.as_bytes(),
                &goal_amount.to_be_bytes(),
                &current_time.to_be_bytes(),
                &new_counter.to_be_bytes(),
            ]
        );

        let end_time = current_time + (duration_days * 24 * 60 * 60);

        let campaign = Campaign {
            id: campaign_id,
            title,
            description,
            beneficiary: beneficiary.clone(),
            goal_amount,
            current_amount: 0,
            start_time: current_time,
            end_time,
            verified: false,
            trust_score: 0,
            category,
            location,
            active: true,
        };

        env.storage().persistent().set(&DataKey::Campaign(campaign_id), &campaign);

        // Update stats
        Self::update_stats(&env, |stats| {
            stats.total_campaigns += 1;
            stats.active_campaigns += 1;
        });

        // Initialize trust score if not exists
        if !env.storage().persistent().has(&DataKey::TrustScore(beneficiary.clone())) {
            Self::create_trust_score(&env, beneficiary.clone())?;
        }

        // Update beneficiary's trust score
        Self::update_beneficiary_trust_score(&env, beneficiary)?;

        env.events().publish(
            (symbol_short!("campaign"), symbol_short!("created")),
            (campaign_id, beneficiary)
        );
        
        Ok(campaign_id)
    }

    /// Get campaign details
    pub fn get_campaign(env: Env, campaign_id: BytesN<32>) -> Option<Campaign> {
        env.storage().persistent().get(&DataKey::Campaign(campaign_id))
    }

    /// Verify a campaign (admin function)
    pub fn verify_campaign(
        env: Env,
        campaign_id: BytesN<32>,
        trust_score: u32,
    ) -> Result<(), SaviaError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin)
            .ok_or(SaviaError::Unauthorized)?;
        admin.require_auth();

        let mut campaign: Campaign = env.storage().persistent().get(&DataKey::Campaign(campaign_id))
            .ok_or(SaviaError::CampaignNotFound)?;

        campaign.verified = true;
        campaign.trust_score = trust_score.min(100);

        env.storage().persistent().set(&DataKey::Campaign(campaign_id), &campaign);
        
        env.events().publish(
            (symbol_short!("campaign"), symbol_short!("verified")),
            (campaign_id, trust_score)
        );
        
        Ok(())
    }

    /// Process a donation
    pub fn donate(
        env: Env,
        campaign_id: BytesN<32>,
        donor: Address,
        amount: u64,
        anonymous: bool,
        mint_nft: bool,
    ) -> Result<BytesN<32>, SaviaError> {
        donor.require_auth();

        // Validate campaign exists and is active
        let mut campaign: Campaign = env.storage().persistent().get(&DataKey::Campaign(campaign_id))
            .ok_or(SaviaError::CampaignNotFound)?;

        if !campaign.active {
            return Err(SaviaError::CampaignInactive);
        }

        let current_time = env.ledger().timestamp();
        if current_time > campaign.end_time {
            return Err(SaviaError::CampaignEnded);
        }

        if amount == 0 {
            return Err(SaviaError::InvalidAmount);
        }

        // Calculate platform fee
        let platform_fee_rate: u64 = env.storage().instance().get(&DataKey::PlatformFee).unwrap_or(200);
        let platform_fee = (amount * platform_fee_rate) / 10000;
        let net_amount = amount - platform_fee;

        // Get and increment donation counter
        let counter: u64 = env.storage().instance().get(&DataKey::DonationCounter).unwrap_or(0);
        let new_counter = counter + 1;
        env.storage().instance().set(&DataKey::DonationCounter, &new_counter);

        // Generate donation ID
        let donation_id = Self::generate_id(
            &env,
            &[
                campaign_id.to_array().as_slice(),
                donor.to_string().as_bytes(),
                &amount.to_be_bytes(),
                &current_time.to_be_bytes(),
                &new_counter.to_be_bytes(),
            ]
        );

        // Create donation record
        let donation = Donation {
            id: donation_id,
            campaign_id,
            donor: donor.clone(),
            amount: net_amount,
            timestamp: current_time,
            nft_minted: mint_nft,
            anonymous,
        };

        // Update campaign progress
        campaign.current_amount += net_amount;
        env.storage().persistent().set(&DataKey::Campaign(campaign_id), &campaign);

        // Store donation
        env.storage().persistent().set(&DataKey::Donation(donation_id), &donation);

        // Update stats
        Self::update_stats(&env, |stats| {
            stats.total_donations += 1;
            stats.total_raised += net_amount;
        });

        // Update trust score
        Self::update_donor_trust_score(&env, donor.clone(), net_amount)?;

        // Mint NFT if requested
        if mint_nft {
            Self::mint_donation_nft(&env, donor.clone(), campaign_id, donation_id, net_amount)?;
        }

        env.events().publish(
            (symbol_short!("donation"), symbol_short!("made")),
            (donation_id, campaign_id, donor, net_amount)
        );

        Ok(donation_id)
    }

    /// Get donation details
    pub fn get_donation(env: Env, donation_id: BytesN<32>) -> Option<Donation> {
        env.storage().persistent().get(&DataKey::Donation(donation_id))
    }

    /// Create trust score for new user
    fn create_trust_score(env: &Env, entity: Address) -> Result<(), SaviaError> {
        if env.storage().persistent().has(&DataKey::TrustScore(entity.clone())) {
            return Err(SaviaError::ScoreExists);
        }

        let trust_score = TrustScore {
            entity: entity.clone(),
            score: 50, // Start with neutral score
            verification_level: 0,
            donation_count: 0,
            total_donated: 0,
            campaigns_created: 0,
            last_updated: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&DataKey::TrustScore(entity), &trust_score);
        Ok(())
    }

    /// Update donor trust score
    fn update_donor_trust_score(env: &Env, donor: Address, amount: u64) -> Result<(), SaviaError> {
        let mut trust_score: TrustScore = env.storage().persistent().get(&DataKey::TrustScore(donor.clone()))
            .unwrap_or_else(|| TrustScore {
                entity: donor.clone(),
                score: 50,
                verification_level: 0,
                donation_count: 0,
                total_donated: 0,
                campaigns_created: 0,
                last_updated: env.ledger().timestamp(),
            });

        trust_score.donation_count += 1;
        trust_score.total_donated += amount;
        trust_score.last_updated = env.ledger().timestamp();

        // Calculate new score based on donation history
        let donation_factor = trust_score.donation_count.min(100) as u64;
        let amount_factor = (trust_score.total_donated / 1000).min(100);
        let consistency_bonus = if trust_score.donation_count > 1 { 5 } else { 0 };

        let new_score = 50 + (donation_factor * 30 / 100) + (amount_factor * 15 / 100) + consistency_bonus;
        trust_score.score = new_score.min(100) as u32;

        env.storage().persistent().set(&DataKey::TrustScore(donor), &trust_score);
        Ok(())
    }

    /// Update beneficiary trust score
    fn update_beneficiary_trust_score(env: &Env, beneficiary: Address) -> Result<(), SaviaError> {
        let mut trust_score: TrustScore = env.storage().persistent().get(&DataKey::TrustScore(beneficiary.clone()))
            .unwrap_or_else(|| TrustScore {
                entity: beneficiary.clone(),
                score: 50,
                verification_level: 0,
                donation_count: 0,
                total_donated: 0,
                campaigns_created: 0,
                last_updated: env.ledger().timestamp(),
            });

        trust_score.campaigns_created += 1;
        trust_score.last_updated = env.ledger().timestamp();

        // Slight boost for creating campaigns
        let campaign_factor = trust_score.campaigns_created.min(20) as u64;
        trust_score.score = (trust_score.score as u64 + campaign_factor).min(100) as u32;

        env.storage().persistent().set(&DataKey::TrustScore(beneficiary), &trust_score);
        Ok(())
    }

    /// Get trust score
    pub fn get_trust_score(env: Env, entity: Address) -> Option<TrustScore> {
        env.storage().persistent().get(&DataKey::TrustScore(entity))
    }

    /// Mint donation NFT
    fn mint_donation_nft(
        env: &Env,
        owner: Address,
        campaign_id: BytesN<32>,
        donation_id: BytesN<32>,
        amount: u64,
    ) -> Result<BytesN<32>, SaviaError> {
        // Get and increment NFT counter
        let counter: u64 = env.storage().instance().get(&DataKey::NFTCounter).unwrap_or(0);
        let new_counter = counter + 1;
        env.storage().instance().set(&DataKey::NFTCounter, &new_counter);

        // Generate NFT ID
        let nft_id = Self::generate_id(
            env,
            &[
                owner.to_string().as_bytes(),
                campaign_id.to_array().as_slice(),
                donation_id.to_array().as_slice(),
                &amount.to_be_bytes(),
                &new_counter.to_be_bytes(),
            ]
        );

        let badge_type = Self::get_badge_type(env, amount);

        let nft_badge = NFTBadge {
            id: nft_id,
            owner: owner.clone(),
            badge_type,
            campaign_id: Some(campaign_id),
            minted_at: env.ledger().timestamp(),
            metadata_uri: String::from_str(env, "https://savia.org/nft/metadata"),
        };

        env.storage().persistent().set(&DataKey::NFTBadge(nft_id), &nft_badge);

        // Update stats
        Self::update_stats(env, |stats| {
            stats.total_nfts += 1;
        });

        env.events().publish(
            (symbol_short!("nft"), symbol_short!("minted")),
            (nft_id, owner, campaign_id)
        );

        Ok(nft_id)
    }

    /// Get NFT details
    pub fn get_nft(env: Env, nft_id: BytesN<32>) -> Option<NFTBadge> {
        env.storage().persistent().get(&DataKey::NFTBadge(nft_id))
    }

    /// Create disbursement request
    pub fn create_disbursement(
        env: Env,
        campaign_id: BytesN<32>,
        recipient: Address,
        amount: u64,
        milestone: String,
    ) -> Result<BytesN<32>, SaviaError> {
        let campaign: Campaign = env.storage().persistent().get(&DataKey::Campaign(campaign_id))
            .ok_or(SaviaError::CampaignNotFound)?;

        // Only beneficiary can create disbursements
        campaign.beneficiary.require_auth();

        if amount > campaign.current_amount {
            return Err(SaviaError::InsufficientFunds);
        }

        // Get and increment disbursement counter
        let counter: u64 = env.storage().instance().get(&DataKey::DisbursementCounter).unwrap_or(0);
        let new_counter = counter + 1;
        env.storage().instance().set(&DataKey::DisbursementCounter, &new_counter);

        // Generate disbursement ID
        let disbursement_id = Self::generate_id(
            &env,
            &[
                campaign_id.to_array().as_slice(),
                recipient.to_string().as_bytes(),
                &amount.to_be_bytes(),
                milestone.as_bytes(),
                &new_counter.to_be_bytes(),
            ]
        );

        let disbursement = Disbursement {
            id: disbursement_id,
            campaign_id,
            recipient: recipient.clone(),
            amount,
            milestone,
            status: DisbursementStatus::Pending,
            created_at: env.ledger().timestamp(),
            executed_at: None,
        };

        env.storage().persistent().set(&DataKey::Disbursement(disbursement_id), &disbursement);

        env.events().publish(
            (symbol_short!("disbursement"), symbol_short!("created")),
            (disbursement_id, campaign_id, recipient, amount)
        );

        Ok(disbursement_id)
    }

    /// Approve disbursement (admin function)
    pub fn approve_disbursement(env: Env, disbursement_id: BytesN<32>) -> Result<(), SaviaError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin)
            .ok_or(SaviaError::Unauthorized)?;
        admin.require_auth();

        let mut disbursement: Disbursement = env.storage().persistent().get(&DataKey::Disbursement(disbursement_id))
            .ok_or(SaviaError::DisbursementNotFound)?;

        disbursement.status = DisbursementStatus::Approved;
        env.storage().persistent().set(&DataKey::Disbursement(disbursement_id), &disbursement);

        env.events().publish(
            (symbol_short!("disbursement"), symbol_short!("approved")),
            disbursement_id
        );

        Ok(())
    }

    /// Execute approved disbursement
    pub fn execute_disbursement(env: Env, disbursement_id: BytesN<32>) -> Result<(), SaviaError> {
        let mut disbursement: Disbursement = env.storage().persistent().get(&DataKey::Disbursement(disbursement_id))
            .ok_or(SaviaError::DisbursementNotFound)?;

        // Only the recipient can execute
        disbursement.recipient.require_auth();

        if disbursement.status != DisbursementStatus::Approved {
            return Err(SaviaError::NotApproved);
        }

        disbursement.status = DisbursementStatus::Executed;
        disbursement.executed_at = Some(env.ledger().timestamp());

        env.storage().persistent().set(&DataKey::Disbursement(disbursement_id), &disbursement);

        env.events().publish(
            (symbol_short!("disbursement"), symbol_short!("executed")),
            disbursement_id
        );

        Ok(())
    }

    /// Get disbursement details
    pub fn get_disbursement(env: Env, disbursement_id: BytesN<32>) -> Option<Disbursement> {
        env.storage().persistent().get(&DataKey::Disbursement(disbursement_id))
    }

    /// Get platform statistics
    pub fn get_stats(env: Env) -> PlatformStats {
        env.storage().instance().get(&DataKey::Stats).unwrap_or_else(|| PlatformStats {
            total_campaigns: 0,
            total_donations: 0,
            total_raised: 0,
            total_nfts: 0,
            active_campaigns: 0,
        })
    }

    /// Close campaign (beneficiary can close early)
    pub fn close_campaign(env: Env, campaign_id: BytesN<32>) -> Result<(), SaviaError> {
        let mut campaign: Campaign = env.storage().persistent().get(&DataKey::Campaign(campaign_id))
            .ok_or(SaviaError::CampaignNotFound)?;

        campaign.beneficiary.require_auth();

        if !campaign.active {
            return Err(SaviaError::CampaignInactive);
        }

        campaign.active = false;
        env.storage().persistent().set(&DataKey::Campaign(campaign_id), &campaign);

        // Update stats
        Self::update_stats(&env, |stats| {
            stats.active_campaigns = stats.active_campaigns.saturating_sub(1);
        });

        env.events().publish(
            (symbol_short!("campaign"), symbol_short!("closed")),
            campaign_id
        );

        Ok(())
    }

    // ========== HELPER FUNCTIONS ==========

    /// Generate a unique ID from multiple byte arrays
    fn generate_id(env: &Env, inputs: &[&[u8]]) -> BytesN<32> {
        let mut hash_input = Bytes::new(env);
        for input in inputs {
            hash_input.append(&Bytes::from_slice(env, input));
        }
        env.crypto().sha256(&hash_input).into()
    }

    /// Update platform statistics
    fn update_stats<F>(env: &Env, updater: F) 
    where
        F: FnOnce(&mut PlatformStats),
    {
        let mut stats = env.storage().instance().get(&DataKey::Stats).unwrap_or_else(|| PlatformStats {
            total_campaigns: 0,
            total_donations: 0,
            total_raised: 0,
            total_nfts: 0,
            active_campaigns: 0,
        });
        
        updater(&mut stats);
        env.storage().instance().set(&DataKey::Stats, &stats);
    }

    /// Determine badge type based on donation amount
    fn get_badge_type(env: &Env, amount: u64) -> String {
        if amount < 1000 {
            String::from_str(env, "Bronze Supporter")
        } else if amount < 5000 {
            String::from_str(env, "Silver Supporter")
        } else if amount < 10000 {
            String::from_str(env, "Gold Supporter")
        } else if amount < 50000 {
            String::from_str(env, "Platinum Supporter")
        } else {
            String::from_str(env, "Diamond Supporter")
        }
    }
}

// ========== TESTS ==========

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env};

    #[test]
    fn test_initialize_contract() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);

        let result = client.initialize(&admin, &200);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_campaign() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);

        client.initialize(&admin, &200);

        let result = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign for testing"),
            &10000,
            &30,
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        );

        assert!(result.is_ok());
        let campaign_id = result.unwrap();
        let campaign = client.get_campaign(&campaign_id);
        assert!(campaign.is_some());
        assert_eq!(campaign.unwrap().goal_amount, 10000);
    }

    #[test]
    fn test_donation_flow() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);
        let donor = Address::generate(&env);

        client.initialize(&admin, &200);

        // Create campaign
        let campaign_id = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign for testing"),
            &10000,
            &30,
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        ).unwrap();

        // Make donation
        let donation_id = client.donate(
            &campaign_id,
            &donor,
            &1000,
            &false,
            &true,
        ).unwrap();

        // Verify donation
        let donation = client.get_donation(&donation_id);
        assert!(donation.is_some());
        assert_eq!(donation.unwrap().amount, 980); // 1000 - 2% fee

        // Check campaign updated
        let campaign = client.get_campaign(&campaign_id).unwrap();
        assert_eq!(campaign.current_amount, 980);

        // Check stats updated
        let stats = client.get_stats();
        assert_eq!(stats.total_donations, 1);
        assert_eq!(stats.total_raised, 980);
    }

    #[test]
    fn test_trust_score_updates() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);
        let donor = Address::generate(&env);

        client.initialize(&admin, &200);

        // Create campaign
        let campaign_id = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign for testing"),
            &10000,
            &30,
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        ).unwrap();

        // Make donation
        client.donate(&campaign_id, &donor, &1000, &false, &false).unwrap();

        // Check trust scores exist
        let donor_score = client.get_trust_score(&donor);
        let beneficiary_score = client.get_trust_score(&beneficiary);
        
        assert!(donor_score.is_some());
        assert!(beneficiary_score.is_some());
        
        assert_eq!(donor_score.unwrap().donation_count, 1);
        assert_eq!(beneficiary_score.unwrap().campaigns_created, 1);
    }

    #[test]
    fn test_disbursement_flow() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);
        let donor = Address::generate(&env);
        let recipient = Address::generate(&env);

        client.initialize(&admin, &200);

        // Create campaign and make donation
        let campaign_id = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign for testing"),
            &10000,
            &30,
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        ).unwrap();

        client.donate(&campaign_id, &donor, &5000, &false, &false).unwrap();

        // Create disbursement
        let disbursement_id = client.create_disbursement(
            &campaign_id,
            &recipient,
            &2000,
            &String::from_str(&env, "Equipment purchase"),
        ).unwrap();

        // Approve disbursement
        client.approve_disbursement(&disbursement_id).unwrap();

        // Execute disbursement
        client.execute_disbursement(&disbursement_id).unwrap();

        // Verify disbursement status
        let disbursement = client.get_disbursement(&disbursement_id).unwrap();
        assert_eq!(disbursement.status, DisbursementStatus::Executed);
        assert!(disbursement.executed_at.is_some());
    }

    #[test]
    fn test_nft_minting() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);
        let donor = Address::generate(&env);

        client.initialize(&admin, &200);

        // Create campaign
        let campaign_id = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign for testing"),
            &10000,
            &30,
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        ).unwrap();

        // Make donation with NFT minting
        client.donate(&campaign_id, &donor, &3000, &false, &true).unwrap();

        // Check stats for NFT count
        let stats = client.get_stats();
        assert_eq!(stats.total_nfts, 1);
    }

    #[test]
    fn test_campaign_verification() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);

        client.initialize(&admin, &200);

        // Create campaign
        let campaign_id = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign for testing"),
            &10000,
            &30,
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        ).unwrap();

        // Verify campaign
        client.verify_campaign(&campaign_id, &85).unwrap();

        // Check verification status
        let campaign = client.get_campaign(&campaign_id).unwrap();
        assert!(campaign.verified);
        assert_eq!(campaign.trust_score, 85);
    }

    #[test]
    fn test_campaign_closure() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);

        client.initialize(&admin, &200);

        // Create campaign
        let campaign_id = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign for testing"),
            &10000,
            &30,
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        ).unwrap();

        // Close campaign
        client.close_campaign(&campaign_id).unwrap();

        // Check campaign is inactive
        let campaign = client.get_campaign(&campaign_id).unwrap();
        assert!(!campaign.active);
    }

    #[test]
    fn test_error_handling() {
        let env = Env::default();
        let contract_id = env.register_contract(None, SaviaContract);
        let client = SaviaContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);

        client.initialize(&admin, &200);

        // Test invalid goal amount
        let result = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign for testing"),
            &0, // Invalid goal
            &30,
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        );
        assert!(result.is_err());

        // Test invalid duration
        let result = client.create_campaign(
            &beneficiary,
            &String::from_str(&env, "Test Campaign"),
            &String::from_str(&env, "A test campaign for testing"),
            &10000,
            &0, // Invalid duration
            &String::from_str(&env, "Health"),
            &String::from_str(&env, "Test City"),
        );
        assert!(result.is_err());
    }
}